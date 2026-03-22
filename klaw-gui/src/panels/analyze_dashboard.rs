use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use egui_plot::{GridMark, Legend, Line, Plot, PlotPoints};
use klaw_config::{ConfigStore, ObservabilityConfig};
use klaw_observability::{
    LocalMetricsStore, LocalStoreConfig, SqliteLocalMetricsStore, ToolDashboardSnapshot,
    ToolSampleBucket, ToolStatsQuery, ToolTimeRange,
};
use klaw_util::{default_data_dir, observability_db_path};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct AnalyzeDashboardPanel {
    store: Option<ConfigStore>,
    observability: ObservabilityConfig,
    storage_root: Option<String>,
    loaded: bool,
    query: ToolStatsQuery,
    selected_tool: Option<String>,
    snapshot: Option<ToolDashboardSnapshot>,
    last_error: Option<String>,
    load_rx: Option<Receiver<Result<ToolDashboardSnapshot, String>>>,
    loading: bool,
    last_loaded_at: Option<Instant>,
}

impl AnalyzeDashboardPanel {
    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        if let Ok(store) = ConfigStore::open(None) {
            let snapshot = store.snapshot();
            self.observability = snapshot.config.observability.clone();
            self.storage_root = snapshot.config.storage.root_dir.clone();
            self.store = Some(store);
            self.loaded = true;
        }
    }

    fn sync_config(&mut self) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let snapshot = store.snapshot();
        self.observability = snapshot.config.observability.clone();
        self.storage_root = snapshot.config.storage.root_dir.clone();
    }

    fn data_root_path(&self) -> Option<PathBuf> {
        self.storage_root
            .as_ref()
            .map(PathBuf::from)
            .or_else(default_data_dir)
    }

    fn local_store_config(&self) -> LocalStoreConfig {
        LocalStoreConfig {
            enabled: self.observability.local_store.enabled,
            retention_days: self.observability.local_store.retention_days,
            flush_interval_seconds: self.observability.local_store.flush_interval_seconds,
        }
    }

    fn should_refresh(&self) -> bool {
        !self.loading
            && self
                .last_loaded_at
                .is_none_or(|loaded_at| loaded_at.elapsed() >= AUTO_REFRESH_INTERVAL)
    }

    fn request_load(&mut self) {
        let Some(root) = self.data_root_path() else {
            self.last_error = Some("Unable to resolve local data directory".to_string());
            return;
        };
        let db_path = observability_db_path(root);
        let query = self.query.clone();
        let selected_tool = self.selected_tool.clone();
        let config = self.local_store_config();
        let (tx, rx) = mpsc::channel();
        self.load_rx = Some(rx);
        self.loading = true;

        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    let _ = tx.send(Err(format!("failed to start query runtime: {err}")));
                    return;
                }
            };

            let result = runtime.block_on(async move {
                let store = SqliteLocalMetricsStore::open(&db_path, &config)
                    .await
                    .map_err(|err| err.to_string())?;
                store
                    .query_tool_dashboard_snapshot(&query, selected_tool.as_deref())
                    .await
                    .map_err(|err| err.to_string())
            });
            let _ = tx.send(result);
        });
    }

    fn poll_load(&mut self, notifications: &mut NotificationCenter) {
        let Some(rx) = self.load_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(snapshot)) => {
                self.snapshot = Some(snapshot);
                self.last_loaded_at = Some(Instant::now());
                self.last_error = None;
                self.loading = false;
                self.load_rx = None;
            }
            Ok(Err(err)) => {
                self.last_error = Some(err.clone());
                notifications.error(format!("Analyze Dashboard load failed: {err}"));
                self.loading = false;
                self.load_rx = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.last_error = Some("Analyze Dashboard worker disconnected".to_string());
                self.loading = false;
                self.load_rx = None;
            }
        }
    }

    fn render_controls(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Time Range:");
            for (label, value) in [
                ("1h", ToolTimeRange::LastHour),
                ("24h", ToolTimeRange::Last24Hours),
                ("7d", ToolTimeRange::Last7Days),
            ] {
                if ui
                    .selectable_label(self.query.time_range == value, label)
                    .clicked()
                {
                    self.query.time_range = value;
                    self.query.bucket_width = match value {
                        ToolTimeRange::LastHour => ToolSampleBucket::OneMinute,
                        ToolTimeRange::Last24Hours | ToolTimeRange::Last7Days => {
                            ToolSampleBucket::OneHour
                        }
                    };
                    changed = true;
                }
            }

            ui.separator();
            ui.label("Granularity:");
            for (label, value) in [
                ("1m", ToolSampleBucket::OneMinute),
                ("1h", ToolSampleBucket::OneHour),
            ] {
                if ui
                    .selectable_label(self.query.bucket_width == value, label)
                    .clicked()
                {
                    self.query.bucket_width = value;
                    changed = true;
                }
            }

            ui.separator();
            if ui.button("Refresh").clicked() {
                changed = true;
            }
            if self.loading {
                ui.label("Loading...");
            } else if let Some(last_loaded_at) = self.last_loaded_at {
                ui.label(format!(
                    "Updated {}s ago",
                    last_loaded_at.elapsed().as_secs()
                ));
            }
        });
        changed
    }

    fn render_summary(&self, ui: &mut egui::Ui, snapshot: &ToolDashboardSnapshot) {
        ui.columns(4, |cols| {
            summary_card(
                &mut cols[0],
                "Total Calls",
                snapshot.summary.total_calls.to_string(),
            );
            summary_card(
                &mut cols[1],
                "Success Rate",
                format_percent(snapshot.summary.success_rate),
            );
            summary_card(
                &mut cols[2],
                "Failures",
                snapshot.summary.failures.to_string(),
            );
            summary_card(
                &mut cols[3],
                "Avg Duration",
                format!("{:.1} ms", snapshot.summary.avg_duration_ms),
            );
        });
    }

    fn render_top_tools(&mut self, ui: &mut egui::Ui, snapshot: &ToolDashboardSnapshot) -> bool {
        let mut changed = false;
        ui.columns(2, |cols| {
            cols[0].group(|ui| {
                ui.strong("Top Tools by Calls");
                ui.separator();
                for row in &snapshot.top_by_calls {
                    let selected = self.selected_tool.as_deref() == Some(row.tool_name.as_str());
                    if ui
                        .selectable_label(
                            selected,
                            format!(
                                "{}  calls={}  success={}",
                                row.tool_name,
                                row.calls,
                                format_percent(row.success_rate)
                            ),
                        )
                        .clicked()
                    {
                        self.selected_tool = Some(row.tool_name.clone());
                        changed = true;
                    }
                }
            });

            cols[1].group(|ui| {
                ui.strong("Top Tools by Failure Load");
                ui.separator();
                for row in &snapshot.top_by_failure_rate {
                    let selected = self.selected_tool.as_deref() == Some(row.tool_name.as_str());
                    if ui
                        .selectable_label(
                            selected,
                            format!(
                                "{}  failures={}  calls={}",
                                row.tool_name, row.failures, row.calls
                            ),
                        )
                        .clicked()
                    {
                        self.selected_tool = Some(row.tool_name.clone());
                        changed = true;
                    }
                }
            });
        });
        changed
    }

    fn render_error_breakdown(&self, ui: &mut egui::Ui, snapshot: &ToolDashboardSnapshot) {
        ui.group(|ui| {
            ui.strong(match self.selected_tool.as_deref() {
                Some(tool_name) => format!("Error Breakdown: {tool_name}"),
                None => "Error Breakdown".to_string(),
            });
            ui.separator();
            if snapshot.error_breakdown.is_empty() {
                ui.label("No failures in the selected time range.");
                return;
            }

            for row in &snapshot.error_breakdown {
                ui.horizontal(|ui| {
                    ui.monospace(&row.error_code);
                    ui.add(
                        egui::ProgressBar::new(
                            (row.failures as f32 / max_failures(snapshot) as f32).clamp(0.0, 1.0),
                        )
                        .show_percentage()
                        .text(row.failures.to_string()),
                    );
                });
            }
        });
    }

    fn render_timeseries(&self, ui: &mut egui::Ui, snapshot: &ToolDashboardSnapshot) {
        ui.group(|ui| {
            ui.strong(format!(
                "Success Rate Trend ({})",
                self.query.bucket_width.label()
            ));
            ui.separator();
            if snapshot.timeseries.is_empty() {
                ui.label("No samples in the selected time range.");
                return;
            }

            let success_rate_points: PlotPoints = snapshot
                .timeseries
                .iter()
                .map(|point| {
                    let x = point.bucket_start_unix_ms as f64;
                    let y = point.success_rate * 100.0;
                    [x, y]
                })
                .collect();

            let calls_points: PlotPoints = snapshot
                .timeseries
                .iter()
                .map(|point| {
                    let x = point.bucket_start_unix_ms as f64;
                    let y = point.calls as f64;
                    [x, y]
                })
                .collect();

            let first_ts = snapshot
                .timeseries
                .first()
                .map(|p| p.bucket_start_unix_ms as f64)
                .unwrap_or(0.0);
            let last_ts = snapshot
                .timeseries
                .last()
                .map(|p| p.bucket_start_unix_ms as f64)
                .unwrap_or(0.0);

            Plot::new("success_rate_trend")
                .legend(Legend::default())
                .x_axis_formatter(|x: GridMark, _| format_time_label(x.value as i64))
                .y_axis_formatter(|y: GridMark, _| format!("{:.0}%", y.value))
                .label_formatter(|name, value| {
                    let time = format_time_label(value.x as i64);
                    if name == "Success Rate" {
                        format!("{}\n{}: {:.1}%", time, name, value.y)
                    } else if name == "Calls" {
                        format!("{}\n{}: {:.0}", time, name, value.y)
                    } else if !name.is_empty() {
                        format!("{}\n{}: {:.2}", time, name, value.y)
                    } else {
                        time
                    }
                })
                .include_x(first_ts)
                .include_x(last_ts)
                .include_y(0.0)
                .include_y(100.0)
                .allow_zoom(false)
                .allow_drag(false)
                .allow_scroll(false)
                .allow_double_click_reset(false)
                .allow_boxed_zoom(false)
                .height(200.0)
                .show(ui, |plot_ui| {
                    plot_ui.line(
                        Line::new("Success Rate", success_rate_points)
                            .color(egui::Color32::from_rgb(100, 200, 100))
                            .width(2.0),
                    );
                    plot_ui.line(
                        Line::new("Calls", calls_points)
                            .color(egui::Color32::from_rgb(100, 150, 250))
                            .width(1.5),
                    );
                });

            ui.horizontal(|ui| {
                ui.label("Total: ");
                let total_calls: u64 = snapshot.timeseries.iter().map(|p| p.calls).sum();
                let total_success: u64 = snapshot.timeseries.iter().map(|p| p.successes).sum();
                let avg_rate = if total_calls > 0 {
                    total_success as f64 / total_calls as f64
                } else {
                    0.0
                };
                ui.label(format!(
                    "{} calls, {} avg success rate",
                    total_calls,
                    format_percent(avg_rate)
                ));
            });
        });
    }
}

impl PanelRenderer for AnalyzeDashboardPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_loaded();
        self.sync_config();
        self.poll_load(notifications);

        ui.heading(ctx.tab_title);
        ui.label("Tool-focused analysis from the local observability store");
        ui.separator();

        if !self.observability.enabled {
            ui.label("Observability is disabled. Enable it in the Observability panel first.");
            return;
        }
        if !self.observability.local_store.enabled {
            ui.label("Local analysis store is disabled. Enable it in the Observability panel.");
            return;
        }

        let manual_refresh = self.render_controls(ui);
        if manual_refresh || self.should_refresh() {
            self.request_load();
        }

        if self.loading {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        } else {
            ui.ctx().request_repaint_after(AUTO_REFRESH_INTERVAL);
        }

        if let Some(err) = &self.last_error {
            ui.colored_label(egui::Color32::LIGHT_RED, err);
            ui.separator();
        }

        let Some(snapshot) = self.snapshot.clone() else {
            ui.label("No local metrics yet.");
            return;
        };

        self.render_summary(ui, &snapshot);
        ui.add_space(8.0);
        let tool_changed = self.render_top_tools(ui, &snapshot);
        ui.add_space(8.0);
        self.render_error_breakdown(ui, &snapshot);
        ui.add_space(8.0);
        self.render_timeseries(ui, &snapshot);

        if tool_changed && !self.loading {
            self.request_load();
        }
    }
}

fn summary_card(ui: &mut egui::Ui, title: &str, value: String) {
    ui.group(|ui| {
        ui.strong(title);
        ui.add_space(4.0);
        ui.heading(value);
    });
}

fn format_percent(value: f64) -> String {
    format!("{:.1}%", value * 100.0)
}

fn max_failures(snapshot: &ToolDashboardSnapshot) -> u64 {
    snapshot
        .error_breakdown
        .iter()
        .map(|row| row.failures)
        .max()
        .unwrap_or(1)
}

fn format_time_label(unix_ms: i64) -> String {
    let Ok(timestamp) = OffsetDateTime::from_unix_timestamp_nanos((unix_ms as i128) * 1_000_000)
    else {
        return unix_ms.to_string();
    };
    match timestamp.format(&Rfc3339) {
        Ok(value) => value
            .split('T')
            .nth(1)
            .map(|time| time.trim_end_matches('Z').chars().take(5).collect())
            .unwrap_or(value),
        Err(_) => unix_ms.to_string(),
    }
}

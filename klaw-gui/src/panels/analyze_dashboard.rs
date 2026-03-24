use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use egui_plot::{GridMark, Legend, Line, Plot, PlotPoints};
use klaw_config::{ConfigStore, ObservabilityConfig};
use klaw_observability::{
    LocalMetricsStore, LocalStoreConfig, ModelDashboardSnapshot, ModelStatsQuery,
    SqliteLocalMetricsStore, ToolDashboardSnapshot, ToolSampleBucket, ToolStatsQuery,
    ToolTimeRange,
};
use klaw_util::{default_data_dir, observability_db_path};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum DashboardView {
    #[default]
    Tools,
    Models,
}

struct DashboardLoad {
    tool_snapshot: ToolDashboardSnapshot,
    model_snapshot: ModelDashboardSnapshot,
}

#[derive(Default)]
pub struct AnalyzeDashboardPanel {
    store: Option<ConfigStore>,
    observability: ObservabilityConfig,
    storage_root: Option<String>,
    loaded: bool,
    view: DashboardView,
    tool_query: ToolStatsQuery,
    selected_tool: Option<String>,
    selected_provider: Option<String>,
    selected_model: Option<String>,
    tool_snapshot: Option<ToolDashboardSnapshot>,
    model_snapshot: Option<ModelDashboardSnapshot>,
    last_error: Option<String>,
    load_rx: Option<Receiver<Result<DashboardLoad, String>>>,
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

    fn model_query(&self) -> ModelStatsQuery {
        ModelStatsQuery {
            time_range: self.tool_query.time_range,
            bucket_width: self.tool_query.bucket_width,
            limit: self.tool_query.limit.max(10),
            provider: self.selected_provider.clone(),
            model: self.selected_model.clone(),
        }
    }

    fn request_load(&mut self) {
        let Some(root) = self.data_root_path() else {
            self.last_error = Some("Unable to resolve local data directory".to_string());
            return;
        };
        let db_path = observability_db_path(root);
        let tool_query = self.tool_query.clone();
        let model_query = self.model_query();
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
                let tool_snapshot = store
                    .query_tool_dashboard_snapshot(&tool_query, selected_tool.as_deref())
                    .await
                    .map_err(|err| err.to_string())?;
                let model_snapshot = store
                    .query_model_dashboard_snapshot(&model_query)
                    .await
                    .map_err(|err| err.to_string())?;
                Ok(DashboardLoad {
                    tool_snapshot,
                    model_snapshot,
                })
            });
            let _ = tx.send(result);
        });
    }

    fn poll_load(&mut self, notifications: &mut NotificationCenter) {
        let Some(rx) = self.load_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(load)) => {
                self.tool_snapshot = Some(load.tool_snapshot);
                self.model_snapshot = Some(load.model_snapshot);
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
            ui.label("View:");
            if ui
                .selectable_label(self.view == DashboardView::Tools, "Tools")
                .clicked()
            {
                self.view = DashboardView::Tools;
                changed = true;
            }
            if ui
                .selectable_label(self.view == DashboardView::Models, "Models")
                .clicked()
            {
                self.view = DashboardView::Models;
                changed = true;
            }
            ui.separator();
            ui.label("Time Range:");
            for (label, value) in [
                ("1h", ToolTimeRange::LastHour),
                ("24h", ToolTimeRange::Last24Hours),
                ("7d", ToolTimeRange::Last7Days),
            ] {
                if ui
                    .selectable_label(self.tool_query.time_range == value, label)
                    .clicked()
                {
                    self.tool_query.time_range = value;
                    self.tool_query.bucket_width = match value {
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
                    .selectable_label(self.tool_query.bucket_width == value, label)
                    .clicked()
                {
                    self.tool_query.bucket_width = value;
                    changed = true;
                }
            }

            if matches!(self.view, DashboardView::Models) {
                if let Some(snapshot) = &self.model_snapshot {
                    ui.separator();
                    egui::ComboBox::from_id_salt("analyze-dashboard-provider")
                        .selected_text(self.selected_provider.as_deref().unwrap_or("All Providers"))
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(self.selected_provider.is_none(), "All Providers")
                                .clicked()
                            {
                                self.selected_provider = None;
                                self.selected_model = None;
                                changed = true;
                            }
                            for provider in &snapshot.providers {
                                if ui
                                    .selectable_label(
                                        self.selected_provider.as_deref()
                                            == Some(provider.as_str()),
                                        provider,
                                    )
                                    .clicked()
                                {
                                    self.selected_provider = Some(provider.clone());
                                    self.selected_model = None;
                                    changed = true;
                                }
                            }
                        });

                    egui::ComboBox::from_id_salt("analyze-dashboard-model")
                        .selected_text(self.selected_model.as_deref().unwrap_or("All Models"))
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(self.selected_model.is_none(), "All Models")
                                .clicked()
                            {
                                self.selected_model = None;
                                changed = true;
                            }
                            for model in &snapshot.models {
                                if ui
                                    .selectable_label(
                                        self.selected_model.as_deref() == Some(model.as_str()),
                                        model,
                                    )
                                    .clicked()
                                {
                                    self.selected_model = Some(model.clone());
                                    changed = true;
                                }
                            }
                        });
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

    fn render_tool_summary(&self, ui: &mut egui::Ui, snapshot: &ToolDashboardSnapshot) {
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

    fn render_model_summary(&self, ui: &mut egui::Ui, snapshot: &ModelDashboardSnapshot) {
        ui.columns(4, |cols| {
            summary_card(
                &mut cols[0],
                "Total Requests",
                snapshot.summary.total_requests.to_string(),
            );
            summary_card(
                &mut cols[1],
                "Success Rate",
                format_percent(snapshot.summary.request_success_rate),
            );
            summary_card(
                &mut cols[2],
                "Avg Duration",
                format!("{:.1} ms", snapshot.summary.avg_duration_ms),
            );
            summary_card(
                &mut cols[3],
                "P95 Duration",
                format!("{:.1} ms", snapshot.latency_percentiles.p95_duration_ms),
            );
        });
        ui.add_space(8.0);
        ui.columns(4, |cols| {
            summary_card(
                &mut cols[0],
                "Total Tokens",
                snapshot.summary.total_tokens.to_string(),
            );
            summary_card(
                &mut cols[1],
                "Estimated Cost",
                format_optional_cost(snapshot.summary.estimated_cost_usd),
            );
            summary_card(
                &mut cols[2],
                "Tool Call Rate",
                format_percent(snapshot.summary.tool_call_rate),
            );
            summary_card(
                &mut cols[3],
                "Turn Completion",
                format_percent(snapshot.summary.turn_completion_rate),
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

    fn render_model_rankings(&self, ui: &mut egui::Ui, snapshot: &ModelDashboardSnapshot) {
        let mut by_tokens = snapshot.model_rows.clone();
        by_tokens.sort_by(|left, right| {
            right
                .total_tokens
                .cmp(&left.total_tokens)
                .then_with(|| left.provider.cmp(&right.provider))
                .then_with(|| left.model.cmp(&right.model))
        });
        let mut by_failures = snapshot.model_rows.clone();
        by_failures.sort_by(|left, right| {
            right.failures.cmp(&left.failures).then_with(|| {
                right
                    .request_failure_rate
                    .total_cmp(&left.request_failure_rate)
            })
        });
        let mut by_p95 = snapshot.model_rows.clone();
        by_p95.sort_by(|left, right| right.p95_duration_ms.total_cmp(&left.p95_duration_ms));
        let mut by_cost = snapshot.model_rows.clone();
        by_cost.sort_by(|left, right| {
            right
                .estimated_cost_usd
                .unwrap_or(0.0)
                .total_cmp(&left.estimated_cost_usd.unwrap_or(0.0))
        });

        ui.columns(2, |cols| {
            render_model_list(
                &mut cols[0],
                "Top Models by Requests",
                snapshot.model_rows.iter(),
                |row| {
                    format!(
                        "{}/{}  req={}  success={}",
                        row.provider,
                        row.model,
                        row.requests,
                        format_percent(row.request_success_rate)
                    )
                },
            );
            render_model_list(
                &mut cols[1],
                "Top Models by Token Usage",
                by_tokens.iter(),
                |row| {
                    format!(
                        "{}/{}  tokens={}  avg={:.1}",
                        row.provider, row.model, row.total_tokens, row.avg_total_tokens
                    )
                },
            );
        });
        ui.add_space(8.0);
        ui.columns(2, |cols| {
            render_model_list(
                &mut cols[0],
                "Worst Models by Failure Load",
                by_failures.iter(),
                |row| {
                    format!(
                        "{}/{}  failures={}  timeout={}",
                        row.provider,
                        row.model,
                        row.failures,
                        format_percent(row.timeout_rate)
                    )
                },
            );
            render_model_list(
                &mut cols[1],
                "Highest P95 Latency Models",
                by_p95.iter(),
                |row| {
                    format!(
                        "{}/{}  p95={:.1} ms  avg={:.1} ms",
                        row.provider, row.model, row.p95_duration_ms, row.avg_duration_ms
                    )
                },
            );
        });
        ui.add_space(8.0);
        render_model_list(&mut *ui, "Highest Cost Models", by_cost.iter(), |row| {
            format!(
                "{}/{}  cost={}  cost/success={}",
                row.provider,
                row.model,
                format_optional_cost(row.estimated_cost_usd),
                format_optional_cost(row.cost_per_successful_turn)
            )
        });
    }

    fn render_tool_error_breakdown(&self, ui: &mut egui::Ui, snapshot: &ToolDashboardSnapshot) {
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
                            (row.failures as f32 / max_tool_failures(snapshot) as f32)
                                .clamp(0.0, 1.0),
                        )
                        .show_percentage()
                        .text(row.failures.to_string()),
                    );
                });
            }
        });
    }

    fn render_model_error_breakdown(&self, ui: &mut egui::Ui, snapshot: &ModelDashboardSnapshot) {
        ui.group(|ui| {
            ui.strong("Error Breakdown by Provider/Model");
            ui.separator();
            if snapshot.error_breakdown.is_empty() {
                ui.label("No model request failures in the selected time range.");
                return;
            }
            let max_failures = snapshot
                .error_breakdown
                .iter()
                .map(|row| row.failures)
                .max()
                .unwrap_or(1);
            for row in &snapshot.error_breakdown {
                ui.horizontal(|ui| {
                    ui.monospace(&row.error_code);
                    ui.add(
                        egui::ProgressBar::new(
                            (row.failures as f32 / max_failures as f32).clamp(0.0, 1.0),
                        )
                        .show_percentage()
                        .text(row.failures.to_string()),
                    );
                });
            }
        });
    }

    fn render_tool_timeseries(&self, ui: &mut egui::Ui, snapshot: &ToolDashboardSnapshot) {
        ui.group(|ui| {
            ui.strong(format!(
                "Success Rate Trend ({})",
                self.tool_query.bucket_width.label()
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
                    [
                        point.bucket_start_unix_ms as f64,
                        point.success_rate * 100.0,
                    ]
                })
                .collect();

            let calls_points: PlotPoints = snapshot
                .timeseries
                .iter()
                .map(|point| [point.bucket_start_unix_ms as f64, point.calls as f64])
                .collect();

            show_plot(
                ui,
                "tool_success_rate_trend",
                &[
                    ("Success Rate", success_rate_points, rgb(100, 200, 100), 2.0),
                    ("Calls", calls_points, rgb(100, 150, 250), 1.5),
                ],
                true,
            );
        });
    }

    fn render_model_timeseries(&self, ui: &mut egui::Ui, snapshot: &ModelDashboardSnapshot) {
        ui.group(|ui| {
            ui.strong(format!(
                "Model Trends ({})",
                self.tool_query.bucket_width.label()
            ));
            ui.separator();
            if snapshot.timeseries.is_empty() {
                ui.label("No model samples in the selected time range.");
                return;
            }

            let success_rate: PlotPoints = snapshot
                .timeseries
                .iter()
                .map(|point| {
                    [
                        point.bucket_start_unix_ms as f64,
                        point.request_success_rate * 100.0,
                    ]
                })
                .collect();
            let avg_duration: PlotPoints = snapshot
                .timeseries
                .iter()
                .map(|point| [point.bucket_start_unix_ms as f64, point.avg_duration_ms])
                .collect();
            let p95_duration: PlotPoints = snapshot
                .timeseries
                .iter()
                .map(|point| [point.bucket_start_unix_ms as f64, point.p95_duration_ms])
                .collect();
            let tokens: PlotPoints = snapshot
                .timeseries
                .iter()
                .map(|point| [point.bucket_start_unix_ms as f64, point.total_tokens as f64])
                .collect();
            let tool_call_rate: PlotPoints = snapshot
                .timeseries
                .iter()
                .map(|point| {
                    [
                        point.bucket_start_unix_ms as f64,
                        point.tool_call_rate * 100.0,
                    ]
                })
                .collect();
            let tool_success_rate: PlotPoints = snapshot
                .timeseries
                .iter()
                .map(|point| {
                    [
                        point.bucket_start_unix_ms as f64,
                        point.tool_success_rate * 100.0,
                    ]
                })
                .collect();
            let requests_per_turn: PlotPoints = snapshot
                .timeseries
                .iter()
                .map(|point| {
                    [
                        point.bucket_start_unix_ms as f64,
                        point.avg_requests_per_turn,
                    ]
                })
                .collect();
            let tool_iterations_per_turn: PlotPoints = snapshot
                .timeseries
                .iter()
                .map(|point| {
                    [
                        point.bucket_start_unix_ms as f64,
                        point.avg_tool_iterations_per_turn,
                    ]
                })
                .collect();

            show_plot(
                ui,
                "model_stability_trend",
                &[
                    ("Success Rate", success_rate, rgb(80, 180, 120), 2.0),
                    ("Tool Call Rate", tool_call_rate, rgb(220, 170, 70), 1.5),
                    (
                        "Tool Success Rate",
                        tool_success_rate,
                        rgb(100, 140, 230),
                        1.5,
                    ),
                ],
                true,
            );
            ui.add_space(8.0);
            show_plot(
                ui,
                "model_latency_trend",
                &[
                    ("Avg Duration", avg_duration, rgb(180, 80, 120), 2.0),
                    ("P95 Duration", p95_duration, rgb(220, 80, 80), 1.5),
                ],
                false,
            );
            ui.add_space(8.0);
            show_plot(
                ui,
                "model_efficiency_trend",
                &[
                    ("Token Usage", tokens, rgb(100, 170, 250), 2.0),
                    ("Requests/Turn", requests_per_turn, rgb(110, 210, 180), 1.5),
                    (
                        "Tool Iterations/Turn",
                        tool_iterations_per_turn,
                        rgb(210, 120, 200),
                        1.5,
                    ),
                ],
                false,
            );
        });
    }

    fn render_model_token_composition(&self, ui: &mut egui::Ui, snapshot: &ModelDashboardSnapshot) {
        ui.group(|ui| {
            ui.strong("Token Composition");
            ui.separator();
            let total = snapshot.summary.total_tokens.max(1);
            for (label, value) in [
                ("Input Tokens", snapshot.token_composition.input_tokens),
                ("Output Tokens", snapshot.token_composition.output_tokens),
                (
                    "Cached Input Tokens",
                    snapshot.token_composition.cached_input_tokens,
                ),
                (
                    "Reasoning Tokens",
                    snapshot.token_composition.reasoning_tokens,
                ),
            ] {
                ui.horizontal(|ui| {
                    ui.label(label);
                    ui.add(
                        egui::ProgressBar::new((value as f32 / total as f32).clamp(0.0, 1.0))
                            .show_percentage()
                            .text(value.to_string()),
                    );
                });
            }
        });
    }

    fn render_model_tool_breakdown(&self, ui: &mut egui::Ui, snapshot: &ModelDashboardSnapshot) {
        ui.group(|ui| {
            ui.strong("Selected Model Tool Success Breakdown");
            ui.separator();
            if snapshot.tool_breakdown.is_empty() {
                ui.label("No model-attributed tool data in the selected time range.");
                return;
            }
            for row in &snapshot.tool_breakdown {
                ui.label(format!(
                    "{}  calls={}  success={}  approval={}  avg={:.1} ms",
                    row.tool_name,
                    row.calls,
                    format_percent(row.success_rate),
                    format_percent(row.approval_required_rate),
                    row.avg_duration_ms
                ));
            }
        });
    }

    fn render_tools_view(&mut self, ui: &mut egui::Ui, snapshot: &ToolDashboardSnapshot) -> bool {
        self.render_tool_summary(ui, snapshot);
        ui.add_space(8.0);
        let tool_changed = self.render_top_tools(ui, snapshot);
        ui.add_space(8.0);
        self.render_tool_error_breakdown(ui, snapshot);
        ui.add_space(8.0);
        self.render_tool_timeseries(ui, snapshot);
        tool_changed
    }

    fn render_models_view(&self, ui: &mut egui::Ui, snapshot: &ModelDashboardSnapshot) {
        self.render_model_summary(ui, snapshot);
        ui.add_space(8.0);
        self.render_model_rankings(ui, snapshot);
        ui.add_space(8.0);
        self.render_model_token_composition(ui, snapshot);
        ui.add_space(8.0);
        self.render_model_tool_breakdown(ui, snapshot);
        ui.add_space(8.0);
        self.render_model_error_breakdown(ui, snapshot);
        ui.add_space(8.0);
        self.render_model_timeseries(ui, snapshot);
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
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.heading(ctx.tab_title);
                ui.label("Tool and model analysis from the local observability store");
                ui.separator();

                if !self.observability.enabled {
                    ui.label(
                        "Observability is disabled. Enable it in the Observability panel first.",
                    );
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

                match self.view {
                    DashboardView::Tools => {
                        let Some(snapshot) = self.tool_snapshot.clone() else {
                            ui.label("No local tool metrics yet.");
                            return;
                        };
                        let tool_changed = self.render_tools_view(ui, &snapshot);
                        if tool_changed && !self.loading {
                            self.request_load();
                        }
                    }
                    DashboardView::Models => {
                        let Some(snapshot) = self.model_snapshot.clone() else {
                            ui.label("No local model metrics yet.");
                            return;
                        };
                        if snapshot.summary.total_requests == 0 {
                            ui.label(
                                "No model-level metrics yet. New charts populate from new telemetry.",
                            );
                            return;
                        }
                        self.render_models_view(ui, &snapshot);
                    }
                }
            });
    }
}

fn summary_card(ui: &mut egui::Ui, title: &str, value: String) {
    ui.group(|ui| {
        ui.strong(title);
        ui.add_space(4.0);
        ui.heading(value);
    });
}

fn render_model_list<'a>(
    ui: &mut egui::Ui,
    title: &str,
    rows: impl IntoIterator<Item = &'a klaw_observability::ModelStatsRow>,
    render: impl Fn(&klaw_observability::ModelStatsRow) -> String,
) {
    ui.group(|ui| {
        ui.strong(title);
        ui.separator();
        for row in rows.into_iter().take(5) {
            ui.label(render(row));
        }
    });
}

fn format_percent(value: f64) -> String {
    format!("{:.1}%", value * 100.0)
}

fn format_optional_cost(value: Option<f64>) -> String {
    value
        .map(|cost| format!("${cost:.4}"))
        .unwrap_or_else(|| "N/A".to_string())
}

fn max_tool_failures(snapshot: &ToolDashboardSnapshot) -> u64 {
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

fn show_plot(
    ui: &mut egui::Ui,
    id: &str,
    lines: &[(&str, PlotPoints<'_>, egui::Color32, f32)],
    percent_axis: bool,
) {
    let first_ts = lines
        .iter()
        .find_map(|(_, points, _, _)| points.points().first().map(|point| point.x))
        .unwrap_or(0.0);
    let last_ts = lines
        .iter()
        .find_map(|(_, points, _, _)| points.points().last().map(|point| point.x))
        .unwrap_or(0.0);
    Plot::new(id)
        .legend(Legend::default())
        .x_axis_formatter(|x: GridMark, _| format_time_label(x.value as i64))
        .y_axis_formatter(|y: GridMark, _| {
            if percent_axis {
                format!("{:.0}%", y.value)
            } else {
                format!("{:.1}", y.value)
            }
        })
        .label_formatter(|name, value| {
            let time = format_time_label(value.x as i64);
            if percent_axis {
                format!("{time}\n{name}: {:.1}%", value.y)
            } else {
                format!("{time}\n{name}: {:.2}", value.y)
            }
        })
        .include_x(first_ts)
        .include_x(last_ts)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_double_click_reset(false)
        .allow_boxed_zoom(false)
        .height(220.0)
        .show(ui, |plot_ui| {
            for (name, points, color, width) in lines {
                plot_ui.line(
                    Line::new(*name, points.points())
                        .color(*color)
                        .width(*width),
                );
            }
        });
}

fn rgb(red: u8, green: u8, blue: u8) -> egui::Color32 {
    egui::Color32::from_rgb(red, green, blue)
}

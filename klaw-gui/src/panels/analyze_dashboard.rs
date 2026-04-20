use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use egui_extras::{Column, TableBuilder};
use egui_plot::{GridMark, Legend, Line, Plot, PlotPoints};
use klaw_config::{ConfigStore, ObservabilityConfig};
use klaw_observability::{
    LocalMetricsStore, LocalStoreConfig, ModelDashboardSnapshot, ModelStatsQuery, ModelStatsRow,
    ModelToolBreakdownRow, PriceEntry, PriceTable, SqliteLocalMetricsStore, ToolDashboardSnapshot,
    ToolSampleBucket, ToolStatsQuery, ToolStatsRow, ToolTimeRange,
};
use klaw_util::{default_data_dir, observability_db_path};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const TOP_TOOLS_TABLE_HEIGHT: f32 = 220.0;
const MODEL_RANKING_TABLE_HEIGHT: f32 = 96.0;
const MODEL_TOOL_BREAKDOWN_TABLE_HEIGHT: f32 = 220.0;
const SMOOTH_PLOT_SEGMENTS_PER_INTERVAL: usize = 12;

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

    fn price_table(&self) -> PriceTable {
        self.observability
            .price
            .iter()
            .map(|(provider, models)| {
                (
                    provider.clone(),
                    models
                        .iter()
                        .map(|(model, entry)| {
                            (
                                model.clone(),
                                PriceEntry {
                                    input_rate: entry.input_rate,
                                    output_rate: entry.output_rate,
                                },
                            )
                        })
                        .collect(),
                )
            })
            .collect()
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
        let price_table = self.price_table();
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
                    .query_model_dashboard_snapshot(&model_query, &price_table)
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
                let selected_tool = self.selected_tool.clone();
                changed |= render_top_tool_table(
                    ui,
                    "Top Tools by Calls",
                    &snapshot.top_by_calls,
                    selected_tool.as_deref(),
                    |row| row.calls.to_string(),
                    |row| format_percent(row.success_rate),
                    "Calls",
                    "Success",
                    &mut self.selected_tool,
                );
            });

            cols[1].group(|ui| {
                let selected_tool = self.selected_tool.clone();
                changed |= render_top_tool_table(
                    ui,
                    "Top Tools by Failure Load",
                    &snapshot.top_by_failure_rate,
                    selected_tool.as_deref(),
                    |row| row.failures.to_string(),
                    |row| row.calls.to_string(),
                    "Failures",
                    "Calls",
                    &mut self.selected_tool,
                );
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
            render_model_stats_table(
                &mut cols[0],
                "Top Models by Requests",
                &snapshot.model_rows,
                |row| row.requests.to_string(),
                |row| format_percent(row.request_success_rate),
                "Requests",
                "Success",
            );
            render_model_stats_table(
                &mut cols[1],
                "Top Models by Token Usage",
                &by_tokens,
                |row| row.total_tokens.to_string(),
                |row| format!("{:.1}", row.avg_total_tokens),
                "Tokens",
                "Avg",
            );
        });
        ui.add_space(8.0);
        ui.columns(2, |cols| {
            render_model_stats_table(
                &mut cols[0],
                "Worst Models by Failure Load",
                &by_failures,
                |row| row.failures.to_string(),
                |row| format_percent(row.timeout_rate),
                "Failures",
                "Timeout",
            );
            render_model_stats_table(
                &mut cols[1],
                "Highest P95 Latency Models",
                &by_p95,
                |row| format!("{:.1} ms", row.p95_duration_ms),
                |row| format!("{:.1} ms", row.avg_duration_ms),
                "P95",
                "Avg",
            );
        });
        ui.add_space(8.0);
        render_model_stats_table(
            &mut *ui,
            "Highest Cost Models",
            &by_cost,
            |row| format_optional_cost(row.estimated_cost_usd),
            |row| format_optional_cost(row.cost_per_successful_turn),
            "Cost",
            "Cost/Success",
        );
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

            render_progress_grid(
                ui,
                "tool_error_breakdown_grid",
                snapshot.error_breakdown.iter().map(|row| {
                    (
                        egui::RichText::new(&row.error_code).monospace(),
                        egui::ProgressBar::new(
                            (row.failures as f32 / max_tool_failures(snapshot) as f32)
                                .clamp(0.0, 1.0),
                        )
                        .show_percentage()
                        .text(row.failures.to_string()),
                    )
                }),
            );
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
            render_progress_grid(
                ui,
                "model_error_breakdown_grid",
                snapshot.error_breakdown.iter().map(|row| {
                    (
                        egui::RichText::new(&row.error_code).monospace(),
                        egui::ProgressBar::new(
                            (row.failures as f32 / max_failures as f32).clamp(0.0, 1.0),
                        )
                        .show_percentage()
                        .text(row.failures.to_string()),
                    )
                }),
            );
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
            render_progress_grid(
                ui,
                "model_token_composition_grid",
                [
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
                ]
                .into_iter()
                .map(|(label, value)| {
                    (
                        egui::WidgetText::from(label),
                        egui::ProgressBar::new((value as f32 / total as f32).clamp(0.0, 1.0))
                            .show_percentage()
                            .text(value.to_string()),
                    )
                }),
            );
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
            render_model_tool_breakdown_table(ui, &snapshot.tool_breakdown);
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

fn render_progress_grid<I, T>(ui: &mut egui::Ui, id_salt: &str, rows: I)
where
    I: IntoIterator<Item = (T, egui::ProgressBar)>,
    T: Into<egui::WidgetText>,
{
    let row_height = ui.spacing().interact_size.y;
    egui::Grid::new(id_salt)
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            for (label, bar) in rows {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(label);
                });
                let bar_width = ui.available_width().max(0.0);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add_sized([bar_width, row_height], bar.desired_height(row_height));
                });
                ui.end_row();
            }
        });
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

fn render_model_stats_table<FPrimary, FSecondary>(
    ui: &mut egui::Ui,
    title: &str,
    rows: &[ModelStatsRow],
    primary_value: FPrimary,
    secondary_value: FSecondary,
    primary_label: &str,
    secondary_label: &str,
) where
    FPrimary: Fn(&ModelStatsRow) -> String,
    FSecondary: Fn(&ModelStatsRow) -> String,
{
    ui.group(|ui| {
        ui.strong(title);
        ui.separator();

        if rows.is_empty() {
            ui.add_sized(
                [ui.available_width(), MODEL_RANKING_TABLE_HEIGHT],
                egui::Label::new("No model data."),
            );
            return;
        }

        ui.push_id(title, |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::remainder().at_least(220.0))
                .column(Column::auto().at_least(84.0))
                .column(Column::auto().at_least(96.0))
                .min_scrolled_height(MODEL_RANKING_TABLE_HEIGHT)
                .max_scroll_height(MODEL_RANKING_TABLE_HEIGHT)
                .header(22.0, |mut header| {
                    header.col(|ui| {
                        ui.strong("Model");
                    });
                    header.col(|ui| {
                        ui.strong(primary_label);
                    });
                    header.col(|ui| {
                        ui.strong(secondary_label);
                    });
                })
                .body(|body| {
                    body.rows(22.0, rows.len(), |mut row| {
                        let item = &rows[row.index()];
                        row.col(|ui| {
                            ui.label(format!("{}/{}", item.provider, item.model));
                        });
                        row.col(|ui| {
                            ui.label(primary_value(item));
                        });
                        row.col(|ui| {
                            ui.label(secondary_value(item));
                        });
                    });
                });
        });
    });
}

fn render_model_tool_breakdown_table(ui: &mut egui::Ui, rows: &[ModelToolBreakdownRow]) {
    ui.push_id("selected-model-tool-breakdown", |ui| {
        TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::remainder().at_least(160.0))
            .column(Column::auto().at_least(72.0))
            .column(Column::auto().at_least(78.0))
            .column(Column::auto().at_least(84.0))
            .column(Column::auto().at_least(88.0))
            .min_scrolled_height(MODEL_TOOL_BREAKDOWN_TABLE_HEIGHT)
            .max_scroll_height(MODEL_TOOL_BREAKDOWN_TABLE_HEIGHT)
            .header(22.0, |mut header| {
                header.col(|ui| {
                    ui.strong("Tool");
                });
                header.col(|ui| {
                    ui.strong("Calls");
                });
                header.col(|ui| {
                    ui.strong("Success");
                });
                header.col(|ui| {
                    ui.strong("Approval");
                });
                header.col(|ui| {
                    ui.strong("Avg");
                });
            })
            .body(|body| {
                body.rows(22.0, rows.len(), |mut row| {
                    let item = &rows[row.index()];
                    row.col(|ui| {
                        ui.label(&item.tool_name);
                    });
                    row.col(|ui| {
                        ui.label(item.calls.to_string());
                    });
                    row.col(|ui| {
                        ui.label(format_percent(item.success_rate));
                    });
                    row.col(|ui| {
                        ui.label(format_percent(item.approval_required_rate));
                    });
                    row.col(|ui| {
                        ui.label(format!("{:.1} ms", item.avg_duration_ms));
                    });
                });
            });
    });
}

fn render_top_tool_table<FPrimary, FSecondary>(
    ui: &mut egui::Ui,
    title: &str,
    rows: &[ToolStatsRow],
    selected_tool: Option<&str>,
    primary_value: FPrimary,
    secondary_value: FSecondary,
    primary_label: &str,
    secondary_label: &str,
    selected_tool_state: &mut Option<String>,
) -> bool
where
    FPrimary: Fn(&ToolStatsRow) -> String,
    FSecondary: Fn(&ToolStatsRow) -> String,
{
    ui.strong(title);
    ui.separator();

    if rows.is_empty() {
        ui.add_sized(
            [ui.available_width(), TOP_TOOLS_TABLE_HEIGHT],
            egui::Label::new("No tool data."),
        );
        return false;
    }

    let mut changed = false;
    ui.push_id(title, |ui| {
        TableBuilder::new(ui)
            .striped(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::remainder().at_least(160.0))
            .column(Column::auto().at_least(72.0))
            .column(Column::auto().at_least(72.0))
            .min_scrolled_height(TOP_TOOLS_TABLE_HEIGHT)
            .max_scroll_height(TOP_TOOLS_TABLE_HEIGHT)
            .header(22.0, |mut header| {
                header.col(|ui| {
                    ui.strong("Tool");
                });
                header.col(|ui| {
                    ui.strong(primary_label);
                });
                header.col(|ui| {
                    ui.strong(secondary_label);
                });
            })
            .body(|body| {
                body.rows(22.0, rows.len(), |mut row| {
                    let item = &rows[row.index()];
                    let is_selected = selected_tool == Some(item.tool_name.as_str());
                    row.set_selected(is_selected);

                    row.col(|ui| {
                        ui.label(&item.tool_name);
                    });
                    row.col(|ui| {
                        ui.label(primary_value(item));
                    });
                    row.col(|ui| {
                        ui.label(secondary_value(item));
                    });

                    if row.response().clicked() {
                        *selected_tool_state = if is_selected {
                            None
                        } else {
                            Some(item.tool_name.clone())
                        };
                        changed = true;
                    }
                });
            });
    });

    changed
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
                    Line::new(*name, smooth_plot_points(points))
                        .color(*color)
                        .width(*width),
                );
            }
        });
}

fn smooth_plot_points(points: &PlotPoints<'_>) -> PlotPoints<'static> {
    let normalized = normalize_plot_points(points);
    if normalized.len() < 3 {
        return normalized.into();
    }

    let x_deltas: Vec<f64> = normalized
        .windows(2)
        .map(|window| window[1][0] - window[0][0])
        .collect();
    let secants: Vec<f64> = normalized
        .windows(2)
        .zip(x_deltas.iter().copied())
        .map(|(window, dx)| (window[1][1] - window[0][1]) / dx)
        .collect();
    let tangents = monotone_cubic_tangents(&x_deltas, &secants);

    let mut smoothed =
        Vec::with_capacity((normalized.len() - 1) * SMOOTH_PLOT_SEGMENTS_PER_INTERVAL + 1);
    smoothed.push(normalized[0]);

    for segment_index in 0..(normalized.len() - 1) {
        let [x0, y0] = normalized[segment_index];
        let [x1, y1] = normalized[segment_index + 1];
        let dx = x1 - x0;
        let m0 = tangents[segment_index];
        let m1 = tangents[segment_index + 1];

        for step in 1..=SMOOTH_PLOT_SEGMENTS_PER_INTERVAL {
            let t = step as f64 / SMOOTH_PLOT_SEGMENTS_PER_INTERVAL as f64;
            let t2 = t * t;
            let t3 = t2 * t;
            let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
            let h10 = t3 - 2.0 * t2 + t;
            let h01 = -2.0 * t3 + 3.0 * t2;
            let h11 = t3 - t2;
            let x = x0 + dx * t;
            let y = h00 * y0 + h10 * dx * m0 + h01 * y1 + h11 * dx * m1;

            if x.is_finite() && y.is_finite() {
                smoothed.push([x, y]);
            }
        }
    }

    smoothed.into()
}

fn normalize_plot_points(points: &PlotPoints<'_>) -> Vec<[f64; 2]> {
    let mut normalized: Vec<[f64; 2]> = Vec::new();
    for point in points.points() {
        if !(point.x.is_finite() && point.y.is_finite()) {
            continue;
        }

        match normalized.last_mut() {
            Some(last) if (point.x - last[0]).abs() <= f64::EPSILON => last[1] = point.y,
            Some(last) if point.x < last[0] => {
                return points.points().iter().map(|p| [p.x, p.y]).collect();
            }
            _ => normalized.push([point.x, point.y]),
        }
    }
    normalized
}

fn monotone_cubic_tangents(x_deltas: &[f64], secants: &[f64]) -> Vec<f64> {
    let point_count = secants.len() + 1;
    let mut tangents = vec![0.0; point_count];
    tangents[0] = secants[0];
    tangents[point_count - 1] = secants[point_count - 2];

    for index in 1..(point_count - 1) {
        tangents[index] = (secants[index - 1] + secants[index]) / 2.0;
    }

    for index in 0..secants.len() {
        let secant = secants[index];
        if secant.abs() <= f64::EPSILON {
            tangents[index] = 0.0;
            tangents[index + 1] = 0.0;
            continue;
        }

        if tangents[index].signum() != secant.signum() {
            tangents[index] = 0.0;
        }
        if tangents[index + 1].signum() != secant.signum() {
            tangents[index + 1] = 0.0;
        }

        let left = tangents[index] / secant;
        let right = tangents[index + 1] / secant;
        let magnitude = (left * left + right * right).sqrt();
        if magnitude > 3.0 {
            let scale = 3.0 / magnitude;
            tangents[index] = scale * left * secant;
            tangents[index + 1] = scale * right * secant;
        }
    }

    for (index, dx) in x_deltas.iter().copied().enumerate() {
        if dx <= f64::EPSILON || !dx.is_finite() {
            tangents[index] = 0.0;
            tangents[index + 1] = 0.0;
        }
    }

    tangents
}

fn rgb(red: u8, green: u8, blue: u8) -> egui::Color32 {
    egui::Color32::from_rgb(red, green, blue)
}

#[cfg(test)]
mod tests {
    use super::{SMOOTH_PLOT_SEGMENTS_PER_INTERVAL, normalize_plot_points, smooth_plot_points};
    use egui_plot::PlotPoints;

    fn plot_pairs(points: &PlotPoints<'_>) -> Vec<[f64; 2]> {
        points
            .points()
            .iter()
            .map(|point| [point.x, point.y])
            .collect()
    }

    #[test]
    fn smooth_plot_points_preserves_small_series() {
        let original = PlotPoints::from(vec![[0.0, 10.0], [1.0, 20.0]]);
        let smoothed = smooth_plot_points(&original);
        assert_eq!(plot_pairs(&smoothed), vec![[0.0, 10.0], [1.0, 20.0]]);
    }

    #[test]
    fn smooth_plot_points_adds_interpolated_samples() {
        let original = PlotPoints::from(vec![[0.0, 0.0], [1.0, 10.0], [2.0, 5.0]]);
        let smoothed = smooth_plot_points(&original);
        let pairs = plot_pairs(&smoothed);

        assert_eq!(pairs.first(), Some(&[0.0, 0.0]));
        assert_eq!(pairs.last(), Some(&[2.0, 5.0]));
        assert_eq!(pairs.len(), (3 - 1) * SMOOTH_PLOT_SEGMENTS_PER_INTERVAL + 1);
    }

    #[test]
    fn normalize_plot_points_collapses_duplicate_timestamps() {
        let original = PlotPoints::from(vec![[0.0, 10.0], [0.0, 12.0], [1.0, 20.0]]);
        assert_eq!(
            normalize_plot_points(&original),
            vec![[0.0, 12.0], [1.0, 20.0]]
        );
    }

    #[test]
    fn smooth_plot_points_does_not_overshoot_monotonic_data() {
        let original = PlotPoints::from(vec![[0.0, 0.0], [1.0, 20.0], [3.0, 40.0], [6.0, 60.0]]);
        let smoothed = smooth_plot_points(&original);

        for [_, y] in plot_pairs(&smoothed) {
            assert!((0.0..=60.0).contains(&y));
        }
    }
}

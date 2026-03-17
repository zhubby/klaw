use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_millis;
use klaw_memory::{MemoryError, MemoryStats, SqliteMemoryStatsService};
use std::future::Future;
use std::thread;
use tokio::runtime::Builder;

#[derive(Default)]
pub struct MemoryPanel {
    loaded: bool,
    stats: Option<MemoryStats>,
}

impl MemoryPanel {
    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        self.refresh(notifications);
    }

    fn refresh(&mut self, notifications: &mut NotificationCenter) {
        match run_memory_task(|service| async move { service.collect(8).await }) {
            Ok(stats) => {
                self.stats = Some(stats);
                self.loaded = true;
            }
            Err(err) => notifications.error(format!("Failed to load memory stats: {err}")),
        }
    }
}

impl PanelRenderer for MemoryPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_loaded(notifications);

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh(notifications);
            }
        });
        ui.separator();

        let Some(stats) = self.stats.as_ref() else {
            ui.label("No memory stats available.");
            return;
        };

        egui::Grid::new("memory-stats-grid")
            .num_columns(2)
            .spacing([14.0, 8.0])
            .show(ui, |ui| {
                ui.label("Total Records");
                ui.monospace(stats.total_records.to_string());
                ui.end_row();

                ui.label("Pinned Records");
                ui.monospace(stats.pinned_records.to_string());
                ui.end_row();

                ui.label("Embedded Records");
                ui.monospace(stats.embedded_records.to_string());
                ui.end_row();

                ui.label("Distinct Scopes");
                ui.monospace(stats.distinct_scopes.to_string());
                ui.end_row();

                ui.label("Updated Last 24h");
                ui.monospace(stats.updated_last_24h.to_string());
                ui.end_row();

                ui.label("Updated Last 7d");
                ui.monospace(stats.updated_last_7d.to_string());
                ui.end_row();

                ui.label("FTS Enabled");
                ui.monospace(if stats.fts_enabled { "yes" } else { "no" });
                ui.end_row();

                ui.label("Vector Index Enabled");
                ui.monospace(if stats.vector_index_enabled {
                    "yes"
                } else {
                    "no"
                });
                ui.end_row();

                ui.label("Avg Content Length");
                ui.monospace(
                    stats
                        .avg_content_len
                        .map(|value| format!("{value:.2}"))
                        .unwrap_or_else(|| "-".to_string()),
                );
                ui.end_row();

                ui.label("Created Min");
                ui.monospace(
                    stats
                        .created_min_ms
                        .map(format_timestamp_millis)
                        .unwrap_or_else(|| "-".to_string()),
                );
                ui.end_row();

                ui.label("Created Max");
                ui.monospace(
                    stats
                        .created_max_ms
                        .map(format_timestamp_millis)
                        .unwrap_or_else(|| "-".to_string()),
                );
                ui.end_row();

                ui.label("Updated Max");
                ui.monospace(
                    stats
                        .updated_max_ms
                        .map(format_timestamp_millis)
                        .unwrap_or_else(|| "-".to_string()),
                );
                ui.end_row();
            });

        ui.separator();
        ui.label("Top Scopes");
        if stats.top_scopes.is_empty() {
            ui.label("No scope data.");
        } else {
            egui::Grid::new("memory-top-scopes-grid")
                .striped(true)
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.strong("Scope");
                    ui.strong("Count");
                    ui.end_row();

                    for scope in &stats.top_scopes {
                        ui.label(&scope.scope);
                        ui.monospace(scope.count.to_string());
                        ui.end_row();
                    }
                });
        }
    }
}

fn run_memory_task<T, F, Fut>(op: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(SqliteMemoryStatsService) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, MemoryError>> + Send + 'static,
{
    let join = thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to build runtime: {err}"))?;

        runtime.block_on(async move {
            let service = SqliteMemoryStatsService::open_default()
                .await
                .map_err(|err| format!("failed to open memory stats service: {err}"))?;
            op(service)
                .await
                .map_err(|err| format!("memory stats operation failed: {err}"))
        })
    });

    match join.join() {
        Ok(result) => result,
        Err(_) => Err("memory stats operation thread panicked".to_string()),
    }
}

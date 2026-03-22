use klaw_storage::SnapshotListItem;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncRuntimeTaskKind {
    StartupCheck,
    AutoBackup,
    ManualBackup,
    RefreshRemoteSnapshots,
    RestoreSnapshot,
    RetentionCleanup,
}

#[derive(Debug, Clone, Default)]
pub struct SyncRuntimeSnapshot {
    pub active_task: Option<SyncRuntimeTask>,
    pub remote_snapshots: Vec<SnapshotListItem>,
    pub remote_update: Option<SnapshotListItem>,
    pub last_snapshot_id: Option<String>,
    pub last_snapshot_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRuntimeTask {
    pub kind: SyncRuntimeTaskKind,
    pub label: String,
}

#[derive(Debug, Default)]
struct SyncRuntimeState {
    active_task: Option<SyncRuntimeTask>,
    remote_snapshots: Vec<SnapshotListItem>,
    remote_update: Option<SnapshotListItem>,
    last_snapshot_id: Option<String>,
    last_snapshot_at: Option<i64>,
}

fn runtime_state() -> &'static Arc<Mutex<SyncRuntimeState>> {
    static STATE: OnceLock<Arc<Mutex<SyncRuntimeState>>> = OnceLock::new();
    STATE.get_or_init(|| Arc::new(Mutex::new(SyncRuntimeState::default())))
}

fn with_runtime_state<T>(f: impl FnOnce(&mut SyncRuntimeState) -> T) -> T {
    let state = runtime_state();
    let mut guard = state.lock().unwrap_or_else(|err| err.into_inner());
    f(&mut guard)
}

pub fn sync_runtime_snapshot() -> SyncRuntimeSnapshot {
    with_runtime_state(|state| SyncRuntimeSnapshot {
        active_task: state.active_task.clone(),
        remote_snapshots: state.remote_snapshots.clone(),
        remote_update: state.remote_update.clone(),
        last_snapshot_id: state.last_snapshot_id.clone(),
        last_snapshot_at: state.last_snapshot_at,
    })
}

pub fn sync_runtime_try_start_task(kind: SyncRuntimeTaskKind, label: impl Into<String>) -> bool {
    with_runtime_state(|state| {
        if state.active_task.is_some() {
            return false;
        }
        state.active_task = Some(SyncRuntimeTask {
            kind,
            label: label.into(),
        });
        true
    })
}

pub fn sync_runtime_finish_task(kind: SyncRuntimeTaskKind) {
    with_runtime_state(|state| {
        if state.active_task.as_ref().map(|task| task.kind) == Some(kind) {
            state.active_task = None;
        }
    });
}

pub fn sync_runtime_set_remote_snapshots(snapshots: Vec<SnapshotListItem>) {
    with_runtime_state(|state| {
        state.remote_snapshots = snapshots;
    });
}

pub fn sync_runtime_set_remote_update(snapshot: Option<SnapshotListItem>) {
    with_runtime_state(|state| {
        state.remote_update = snapshot;
    });
}

pub fn sync_runtime_set_last_snapshot(snapshot_id: Option<String>, created_at: Option<i64>) {
    with_runtime_state(|state| {
        state.last_snapshot_id = snapshot_id;
        state.last_snapshot_at = created_at;
    });
}

pub fn sync_runtime_sync_from_settings(
    last_snapshot_id: Option<String>,
    last_snapshot_at: Option<i64>,
) {
    with_runtime_state(|state| {
        if state.last_snapshot_id != last_snapshot_id {
            state.last_snapshot_id = last_snapshot_id;
        }
        if state.last_snapshot_at != last_snapshot_at {
            state.last_snapshot_at = last_snapshot_at;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_task_blocks_parallel_work() {
        sync_runtime_finish_task(SyncRuntimeTaskKind::ManualBackup);
        assert!(sync_runtime_try_start_task(
            SyncRuntimeTaskKind::ManualBackup,
            "Manual backup"
        ));
        assert!(!sync_runtime_try_start_task(
            SyncRuntimeTaskKind::AutoBackup,
            "Auto backup"
        ));
        sync_runtime_finish_task(SyncRuntimeTaskKind::ManualBackup);
        assert!(sync_runtime_try_start_task(
            SyncRuntimeTaskKind::AutoBackup,
            "Auto backup"
        ));
        sync_runtime_finish_task(SyncRuntimeTaskKind::AutoBackup);
    }
}

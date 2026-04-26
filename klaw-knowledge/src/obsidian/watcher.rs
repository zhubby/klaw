use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use async_trait::async_trait;
use notify::{
    Event, EventKind, RecursiveMode,
    event::{ModifyKind, RenameMode},
};
use notify_debouncer_full::{DebouncedEvent, new_debouncer};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use crate::{KnowledgeAutoIndexHandle, KnowledgeError, ObsidianKnowledgeProvider};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    Changed(PathBuf),
    Deleted(PathBuf),
    Moved { from: PathBuf, to: PathBuf },
    FullRescan,
}

pub struct AutoIndexWatcher {
    shutdown_tx: Option<oneshot::Sender<()>>,
    producer: Option<std::thread::JoinHandle<()>>,
    consumer: Option<JoinHandle<()>>,
}

impl AutoIndexWatcher {
    pub async fn stop(mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        if let Some(producer) = self.producer.take() {
            let _ = tokio::task::spawn_blocking(move || producer.join()).await;
        }
        if let Some(consumer) = self.consumer.take() {
            consumer.abort();
            let _ = consumer.await;
        }
    }
}

#[async_trait]
impl KnowledgeAutoIndexHandle for AutoIndexWatcher {
    async fn stop(self: Box<Self>) {
        (*self).stop().await;
    }
}

impl Drop for AutoIndexWatcher {
    fn drop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
    }
}

pub fn start_auto_index_watcher(
    provider: ObsidianKnowledgeProvider,
) -> Result<AutoIndexWatcher, KnowledgeError> {
    let vault_root = provider.vault_root().to_path_buf();
    let exclude_folders = provider.exclude_folders().to_vec();
    let (event_tx, event_rx) = mpsc::channel::<Vec<WatchEvent>>(64);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let producer = start_producer(vault_root.clone(), exclude_folders, event_tx, shutdown_rx)?;
    let consumer = tokio::spawn(run_consumer(provider, event_rx));

    Ok(AutoIndexWatcher {
        shutdown_tx: Some(shutdown_tx),
        producer: Some(producer),
        consumer: Some(consumer),
    })
}

fn start_producer(
    vault_root: PathBuf,
    exclude_folders: Vec<String>,
    tx: mpsc::Sender<Vec<WatchEvent>>,
    mut shutdown_rx: oneshot::Receiver<()>,
) -> Result<std::thread::JoinHandle<()>, KnowledgeError> {
    let (debouncer_tx, debouncer_rx) = std::sync::mpsc::channel();
    let mut debouncer =
        new_debouncer(Duration::from_secs(2), None, debouncer_tx).map_err(|err| {
            KnowledgeError::Provider(format!("create knowledge watcher failed: {err}"))
        })?;
    debouncer
        .watch(&vault_root, RecursiveMode::Recursive)
        .map_err(|err| KnowledgeError::Provider(format!("watch knowledge vault failed: {err}")))?;

    Ok(std::thread::spawn(move || {
        let _keep_debouncer_alive = debouncer;
        loop {
            if shutdown_rx.try_recv().is_ok() {
                break;
            }
            match debouncer_rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Ok(events)) => {
                    let watch_events =
                        debounced_to_watch_events(&events, &vault_root, &exclude_folders);
                    if !watch_events.is_empty() && tx.blocking_send(watch_events).is_err() {
                        break;
                    }
                }
                Ok(Err(errors)) => {
                    for err in errors {
                        tracing::warn!(error = ?err, "knowledge watcher event error");
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }))
}

fn debounced_to_watch_events(
    events: &[DebouncedEvent],
    vault_root: &Path,
    exclude_folders: &[String],
) -> Vec<WatchEvent> {
    events
        .iter()
        .flat_map(|event| notify_to_watch_events(&event.event, vault_root, exclude_folders))
        .collect()
}

pub(crate) fn notify_to_watch_events(
    event: &Event,
    vault_root: &Path,
    exclude_folders: &[String],
) -> Vec<WatchEvent> {
    let paths = event
        .paths
        .iter()
        .filter(|path| is_indexable_markdown_path(path, vault_root, exclude_folders))
        .cloned()
        .collect::<Vec<_>>();

    match &event.kind {
        EventKind::Create(_) => paths.into_iter().map(WatchEvent::Changed).collect(),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            let Some(from) = event.paths.first() else {
                return Vec::new();
            };
            let Some(to) = event.paths.get(1) else {
                return Vec::new();
            };
            rename_pair_to_watch_events(from, to, vault_root, exclude_folders)
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            paths.into_iter().map(WatchEvent::Deleted).collect()
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            paths.into_iter().map(WatchEvent::Changed).collect()
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::Any)) => paths
            .into_iter()
            .map(|path| {
                if path.exists() {
                    WatchEvent::Changed(path)
                } else {
                    WatchEvent::Deleted(path)
                }
            })
            .collect(),
        EventKind::Modify(ModifyKind::Name(RenameMode::Other)) => vec![WatchEvent::FullRescan],
        EventKind::Modify(_) => paths.into_iter().map(WatchEvent::Changed).collect(),
        EventKind::Remove(_) => paths.into_iter().map(WatchEvent::Deleted).collect(),
        EventKind::Other => vec![WatchEvent::FullRescan],
        _ => Vec::new(),
    }
}

async fn run_consumer(
    provider: ObsidianKnowledgeProvider,
    mut rx: mpsc::Receiver<Vec<WatchEvent>>,
) {
    if let Err(err) = provider.reconcile_existing_index().await {
        tracing::warn!(error = %err, "knowledge auto-index startup reconciliation failed");
    }

    while let Some(events) = rx.recv().await {
        for event in events {
            if let Err(err) = handle_event(&provider, event).await {
                tracing::warn!(error = %err, "knowledge auto-index event failed");
            }
        }
    }
}

async fn handle_event(
    provider: &ObsidianKnowledgeProvider,
    event: WatchEvent,
) -> Result<(), KnowledgeError> {
    match event {
        WatchEvent::Changed(path) => {
            if path.exists() {
                provider.index_path(&path).await?;
            }
        }
        WatchEvent::Deleted(path) => {
            let relative_path = relative_path(provider.vault_root(), &path)?;
            provider.remove_path(&relative_path).await?;
        }
        WatchEvent::Moved { from, to } => {
            let relative_path = relative_path(provider.vault_root(), &from)?;
            provider.remove_path(&relative_path).await?;
            if to.exists() {
                provider.index_path(&to).await?;
            }
        }
        WatchEvent::FullRescan => {
            provider.reconcile_existing_index().await?;
        }
    }
    Ok(())
}

fn relative_path(vault_root: &Path, path: &Path) -> Result<String, KnowledgeError> {
    Ok(path
        .strip_prefix(vault_root)
        .map_err(|err| KnowledgeError::Provider(format!("strip prefix failed: {err}")))?
        .to_string_lossy()
        .replace('\\', "/"))
}

fn is_excluded(path: &Path, vault_root: &Path, exclude_folders: &[String]) -> bool {
    let relative = path.strip_prefix(vault_root).unwrap_or(path);
    relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        exclude_folders
            .iter()
            .any(|exclude| exclude.trim_matches('/') == name)
    })
}

fn is_indexable_markdown_path(path: &Path, vault_root: &Path, exclude_folders: &[String]) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("md")
        && !is_excluded(path, vault_root, exclude_folders)
}

fn rename_pair_to_watch_events(
    from: &Path,
    to: &Path,
    vault_root: &Path,
    exclude_folders: &[String],
) -> Vec<WatchEvent> {
    let from_indexable = is_indexable_markdown_path(from, vault_root, exclude_folders);
    let to_indexable = is_indexable_markdown_path(to, vault_root, exclude_folders);
    match (from_indexable, to_indexable) {
        (true, true) => vec![WatchEvent::Moved {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
        }],
        (true, false) => vec![WatchEvent::Deleted(from.to_path_buf())],
        (false, true) => vec![WatchEvent::Changed(to.to_path_buf())],
        (false, false) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use notify::{event::CreateKind, event::ModifyKind, event::RemoveKind};

    use super::*;

    #[test]
    fn notify_to_watch_events_filters_markdown_and_excludes() {
        let vault = PathBuf::from("/vault");
        let event = Event::new(EventKind::Create(CreateKind::File))
            .add_path(vault.join("note.md"))
            .add_path(vault.join("note.txt"))
            .add_path(vault.join(".obsidian/ignored.md"));

        let events = notify_to_watch_events(
            &event,
            &vault,
            &[".obsidian".to_string(), "templates".to_string()],
        );

        assert_eq!(events, vec![WatchEvent::Changed(vault.join("note.md"))]);
    }

    #[test]
    fn notify_to_watch_events_maps_modify_remove_and_other() {
        let vault = PathBuf::from("/vault");
        let modify = Event::new(EventKind::Modify(ModifyKind::Data(
            notify::event::DataChange::Content,
        )))
        .add_path(vault.join("changed.md"));
        let remove =
            Event::new(EventKind::Remove(RemoveKind::File)).add_path(vault.join("deleted.md"));
        let other = Event::new(EventKind::Other);

        assert_eq!(
            notify_to_watch_events(&modify, &vault, &[]),
            vec![WatchEvent::Changed(vault.join("changed.md"))]
        );
        assert_eq!(
            notify_to_watch_events(&remove, &vault, &[]),
            vec![WatchEvent::Deleted(vault.join("deleted.md"))]
        );
        assert_eq!(
            notify_to_watch_events(&other, &vault, &[]),
            vec![WatchEvent::FullRescan]
        );
    }

    #[test]
    fn notify_to_watch_events_maps_rename_events() {
        let vault = PathBuf::from("/vault");
        let rename_both = Event::new(EventKind::Modify(ModifyKind::Name(
            notify::event::RenameMode::Both,
        )))
        .add_path(vault.join("old.md"))
        .add_path(vault.join("new.md"));
        let rename_from = Event::new(EventKind::Modify(ModifyKind::Name(
            notify::event::RenameMode::From,
        )))
        .add_path(vault.join("old.md"));
        let rename_to = Event::new(EventKind::Modify(ModifyKind::Name(
            notify::event::RenameMode::To,
        )))
        .add_path(vault.join("new.md"));

        assert_eq!(
            notify_to_watch_events(&rename_both, &vault, &[]),
            vec![WatchEvent::Moved {
                from: vault.join("old.md"),
                to: vault.join("new.md"),
            }]
        );
        assert_eq!(
            notify_to_watch_events(&rename_from, &vault, &[]),
            vec![WatchEvent::Deleted(vault.join("old.md"))]
        );
        assert_eq!(
            notify_to_watch_events(&rename_to, &vault, &[]),
            vec![WatchEvent::Changed(vault.join("new.md"))]
        );
    }
}

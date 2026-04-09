# klaw-ui-kit

`klaw-ui-kit` contains UI foundation shared by `klaw-gui` and `klaw-webui`.

## What belongs here

- Shared theme enums and labels
- Shared font installation and embedded font assets used by both frontends
- Platform-agnostic display copy helpers
- Lightweight `egui` wrappers used by both frontends

## What does not belong here

- App shell or workbench orchestration
- Browser-only transport or `web_sys` integration
- Desktop runtime bridge code
- Feature-specific panels, dialogs, or chat flows

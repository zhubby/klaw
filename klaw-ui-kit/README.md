# klaw-ui-kit

`klaw-ui-kit` contains UI foundation shared by `klaw-gui` and `klaw-webui`.

## What belongs here

- Shared theme enums and labels
- Shared theme widgets, including the tri-state `ThemeSwitch` for `egui::ThemePreference`
- Shared font installation and embedded font assets used by both frontends
- Platform-agnostic display copy helpers
- Lightweight `egui` wrappers used by both frontends

## Font features

`klaw-ui-kit` selects its embedded CJK fonts at compile time with Cargo features:

- Default: `fonts-lxgw`
- Optional: `fonts-noto-sans`
- Allowed fallback mode: disable default features and enable neither font feature to keep `egui` default fonts plus the existing desktop system CJK fallbacks

The two font features are mutually exclusive. Enabling both at once fails compilation.

Examples:

```toml
# Default behavior: embed LXGW WenKai.
klaw-ui-kit = { workspace = true }

# Switch to Noto Sans SC + Noto Sans Mono.
klaw-ui-kit = { workspace = true, default-features = false, features = ["fonts-noto-sans"] }

# Disable embedded fonts entirely.
klaw-ui-kit = { workspace = true, default-features = false }
```

## What does not belong here

- App shell or workbench orchestration
- Browser-only transport or `web_sys` integration
- Desktop runtime bridge code
- Feature-specific panels, dialogs, or chat flows

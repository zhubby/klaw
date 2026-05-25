# klaw-ui-kit

`klaw-ui-kit` contains UI foundation shared by `klaw-gui` and `klaw-webui`.

## What belongs here

- Shared theme enums and labels
- Shared theme widgets, including the tri-state `ThemeSwitch` for `egui::ThemePreference`
- Shared font installation and embedded font assets used by both frontends
- Platform-agnostic display copy helpers
- Shared i18n primitives, language selection types, and embedded Fluent resources for frontend-specific domains
- Lightweight `egui` wrappers used by both frontends, including shared controls and simple chart widgets

## Shared widgets

`klaw-ui-kit` provides platform-agnostic `egui` widgets that can be used from both desktop and web frontends.

### Pie chart

Use `PieChart` with `PieSlice` data for small categorical charts:

```rust
let slices = vec![
    PieSlice::new("Open", 42.0),
    PieSlice::new("Closed", 18.0).color(egui::Color32::from_rgb(214, 39, 40)),
];

ui.add(
    PieChart::new(&slices)
        .palette(PieChartPalette::Tableau)
        .show_labels(true)
        .show_separators(true)
        .desired_size(egui::vec2(220.0, 220.0)),
);
```

For equal fractional slices, use `equal_pie_slices(count)`:

```rust
let slices = equal_pie_slices(6);
ui.add(PieChart::new(&slices));
```

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

## i18n resources

`klaw-ui-kit` provides the shared `UiLanguage`, `LocaleDomain`, and `Translator` APIs used by both frontends. The supported languages are English (`en-US`) and Simplified Chinese (`zh-CN`), with English as the fallback language.

GUI and WebUI copy use separate Fluent domains so each frontend can evolve its own labels without forcing shared keys:

- `locales/{language}/gui.ftl`
- `locales/{language}/webui.ftl`

## What does not belong here

- App shell or workbench orchestration
- Browser-only transport or `web_sys` integration
- Desktop runtime bridge code
- Feature-specific panels, dialogs, or chat flows

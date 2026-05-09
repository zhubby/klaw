//! Benchmarks for the i18n `Translator` used in GUI/WebUI rendering loops.
//!
//! These measure the cost of text lookups that happen on **every egui frame**.
//! The `OnceLock`-based loader cache absorbs the one-time Fluent loading cost,
//! and subsequent calls hit the cached `&'static` loader. Benchmarks run after
//! that warm-up so they measure the steady-state path the UI actually walks.
//!
//! Run with:
//!   cargo bench -p klaw-ui-kit --bench locale_bench
//! HTML reports land in `target/criterion/`.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use klaw_ui_kit::{LocaleDomain, Translator, UiLanguage};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Sidebar keys rendered each frame (6 group titles + 28 menu titles).
// ---------------------------------------------------------------------------
const SIDEBAR_KEYS: &[&str] = &[
    "menu-group-workspace",
    "menu-group-ai-and-capability",
    "menu-group-runtime-and-access",
    "menu-group-automation-and-operations",
    "menu-group-data-and-history",
    "menu-group-observability",
    "menu-profile",
    "menu-system",
    "menu-setting",
    "menu-terminal",
    "menu-session",
    "menu-approval",
    "menu-configuration",
    "menu-provider",
    "menu-local-models",
    "menu-llm",
    "menu-channel",
    "menu-voice",
    "menu-cron",
    "menu-heartbeat",
    "menu-gateway",
    "menu-webhook",
    "menu-mcp",
    "menu-acp",
    "menu-skill-registry",
    "menu-skills-manager",
    "menu-memory",
    "menu-knowledge",
    "menu-archive",
    "menu-tool",
    "menu-monitor",
    "menu-logs",
    "menu-analyze-dashboard",
    "menu-observability",
];

// Parameterized keys used in the status bar and About dialog.
const PARAMETERIZED_KEYS: &[&str] = &[
    "status-default-model",
    "status-update-available",
    "status-update-hover",
    "about-version",
    "about-git-commit",
];

// ---------------------------------------------------------------------------
// Warm-up: ensure `OnceLock` loaders are populated before measuring.
// ---------------------------------------------------------------------------
fn warm_up() {
    for domain in [LocaleDomain::Gui, LocaleDomain::WebUi] {
        for language in UiLanguage::available() {
            let translator = Translator::new(domain, *language);
            let _ = translator.text("menu-file");
        }
    }
}

// ---------------------------------------------------------------------------
// Bench 1: Translator::new() — cached loader lookup per frame.
// ---------------------------------------------------------------------------
fn bench_translator_new(c: &mut Criterion) {
    warm_up();

    let mut group = c.benchmark_group("translator_new");

    for language in UiLanguage::available() {
        group.bench_function(BenchmarkId::new("gui", language.label()), |b| {
            b.iter(|| black_box(Translator::new(LocaleDomain::Gui, black_box(*language))));
        });
        group.bench_function(BenchmarkId::new("webui", language.label()), |b| {
            b.iter(|| black_box(Translator::new(LocaleDomain::WebUi, black_box(*language))));
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Bench 2: Translator::text() — simple key lookup.
// ---------------------------------------------------------------------------
fn bench_text_simple(c: &mut Criterion) {
    warm_up();

    let mut group = c.benchmark_group("text_simple");

    for language in UiLanguage::available() {
        let translator = Translator::new(LocaleDomain::Gui, *language);

        group.bench_function(BenchmarkId::new("single_key", language.label()), |b| {
            b.iter(|| black_box(translator.text(black_box("menu-file"))));
        });

        group.bench_function(BenchmarkId::new("missing_key", language.label()), |b| {
            b.iter(|| black_box(translator.text(black_box("nonexistent-key-xyz"))));
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Bench 3: Translator::text_args() — parameterized key lookup.
// ---------------------------------------------------------------------------
fn bench_text_args(c: &mut Criterion) {
    warm_up();

    let mut group = c.benchmark_group("text_args");

    for language in UiLanguage::available() {
        let translator = Translator::new(LocaleDomain::Gui, *language);

        // Single-argument message: "Default Model: {model}"
        group.bench_function(BenchmarkId::new("single_arg", language.label()), |b| {
            b.iter(|| {
                let mut args = HashMap::new();
                args.insert("model", "gpt-4o".to_string());
                black_box(translator.text_args(black_box("status-default-model"), args));
            });
        });

        // Multi-argument message: status-update-hover with {current}, {latest}, {name}
        group.bench_function(BenchmarkId::new("multi_arg", language.label()), |b| {
            b.iter(|| {
                let mut args = HashMap::new();
                args.insert("current", "0.16.4".to_string());
                args.insert("latest", "0.16.5".to_string());
                args.insert("name", "Klaw Release".to_string());
                black_box(translator.text_args(black_box("status-update-hover"), args));
            });
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Bench 4: Full sidebar simulation — translate all 34 keys per "frame".
// ---------------------------------------------------------------------------
fn bench_sidebar_simulation(c: &mut Criterion) {
    warm_up();

    let mut group = c.benchmark_group("sidebar_full_frame");

    for language in UiLanguage::available() {
        let translator = Translator::new(LocaleDomain::Gui, *language);

        group.bench_function(BenchmarkId::new("all_keys", language.label()), |b| {
            b.iter(|| {
                for key in SIDEBAR_KEYS {
                    black_box(translator.text(black_box(key)));
                }
            });
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Bench 5: Full status-bar + About simulation per "frame".
// ---------------------------------------------------------------------------
fn bench_status_bar_simulation(c: &mut Criterion) {
    warm_up();

    let mut group = c.benchmark_group("status_bar_full_frame");

    let simple_status_keys: &[&str] = &[
        "status-theme-mode",
        "status-model-provider",
        "status-model-provider-na",
        "status-hide-window",
        "status-zoom-window",
        "status-minimize-window",
        "about-title",
        "about-close",
    ];

    for language in UiLanguage::available() {
        let translator = Translator::new(LocaleDomain::Gui, *language);

        group.bench_function(BenchmarkId::new("all_keys", language.label()), |b| {
            b.iter(|| {
                // Simple labels
                for key in simple_status_keys {
                    black_box(translator.text(black_box(key)));
                }

                // Parameterized labels
                for key in PARAMETERIZED_KEYS {
                    let mut args = HashMap::new();
                    match *key {
                        "status-default-model" => {
                            args.insert("model", "gpt-4o".to_string());
                        }
                        "status-update-available" => {
                            args.insert("icon", "⬇".to_string());
                            args.insert("version", "0.16.5".to_string());
                        }
                        "status-update-hover" => {
                            args.insert("current", "0.16.4".to_string());
                            args.insert("latest", "0.16.5".to_string());
                            args.insert("name", "Klaw Release".to_string());
                        }
                        "about-version" => {
                            args.insert("version", "0.16.5".to_string());
                        }
                        "about-git-commit" => {
                            args.insert("sha", "abc123def456".to_string());
                        }
                        _ => {}
                    }
                    black_box(translator.text_args(black_box(key), args));
                }
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_translator_new,
    bench_text_simple,
    bench_text_args,
    bench_sidebar_simulation,
    bench_status_bar_simulation,
);

criterion_main!(benches);

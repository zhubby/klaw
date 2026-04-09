# Repository Guidelines

## Project Structure & Module Organization

This repository is a Rust workspace. Crates are split by responsibility:

- `klaw-core`: agent loop, protocol, reliability, scheduling.
- `klaw-cli`: CLI entrypoint and command handlers (`tui`, `agent`, etc.).
- `klaw-llm`: LLM provider integrations.
- `klaw-tool`: tool implementations and registry.
- `klaw-config`: TOML config loading/validation.
- `klaw-mcp`, `klaw-skill`, `klaw-memory`: MCP, skill, and memory support crates.
- `docs/`: mdBook sources (`docs/src`) and architecture notes (`docs/agent-core`).

Keep new code in the crate that owns the domain concern; avoid cross-crate leakage of CLI-specific logic into core runtime crates.

## Shared UI Extraction

Use `klaw-ui-kit` for UI foundation shared by both `klaw-gui` and `klaw-webui`, but keep the bar high:

- Extract only when the concept already exists in both frontends or the second consumer is immediate and concrete.
- Prefer shared low-level building blocks such as theme enums, display labels, copy helpers, and lightweight `egui` wrappers.
- Do not move app shell, panel orchestration, runtime bridge code, transport logic, or platform-specific lifecycle code into `klaw-ui-kit`.
- If a shared API would need awkward compromise naming or would force one frontend to give up a better UX, keep it local instead.
- Shared UI code must remain platform-agnostic: avoid `web_sys`, browser-only callbacks, desktop runtime bridge types, and other frontend-specific dependencies.

## Build, Test, and Development Commands

Use workspace-level Cargo commands from repo root:

- `cargo check --workspace`: fast compile verification.
- `cargo build --workspace`: build all crates.
- `cargo test --workspace`: run unit and integration tests.
- `cargo test -p klaw-core --test runtime_e2e`: run core E2E runtime tests.
- `cargo fmt --all`: apply Rust formatting.
- `cargo clippy --workspace --all-targets -- -D warnings`: lint strictly.
- `klaw tui`: run interactive local agent loop in the terminal UI.
- `klaw agent --input "hello"`: single request/response run.

For docs: `mdbook build docs` (or `mdbook serve docs` for local preview).

## Rust Style and Idioms

- Target Rust 2024 for new code and examples. Prefer edition-aware idioms, and only use raw identifiers such as `r#gen` when compatibility leaves no cleaner choice.
- Use traits for behaviour boundaries. Prefer generics for hot paths, `dyn Trait` for heterogeneous/runtime dispatch.
- Derive `Default` when all fields have sensible defaults.
- Use concrete types (`struct`/`enum`) over `serde_json::Value` wherever shape is known.
- **Match on types, never strings.** Only convert to strings at serialization/display boundaries.
- Prefer `From`/`Into`/`TryFrom`/`TryInto` over manual conversions. Ask before adding manual conversion paths.
- Prefer streaming over non-streaming API calls.
- Run independent async work concurrently (`tokio::join!`, `futures::join_all`).
- Never use `block_on` inside async context.
- **Forbidden:** `Mutex<()>` / `Arc<Mutex<()>>` — mutex must guard actual state.
- Use `anyhow::Result` for app errors, `thiserror` for library errors. Propagate with `?`.
- **Never `.unwrap()`/`.expect()` in production.** Workspace lints deny these. Use `?`, `ok_or_else`, `unwrap_or_default`, `unwrap_or_else(|e| e.into_inner())` for locks.
- Use `time` crate (workspace dep) for date/time — no manual epoch math or magic constants like `86400`.
- Prefer `chrono` only if already imported in the crate; default to `time` for new code.
- Prefer crates over subprocesses (`std::process::Command`). Use subprocesses only when no mature crate exists.
- Prefer guard clauses (early returns) over nested `if` blocks.
- Prefer `let-else` when destructuring must succeed and the failure path should return, `continue`, or `break`.
- Prefer `if let` chains, `matches!`, and pattern guards over nested single-arm `match` blocks when they make branching flatter and clearer.
- Prefer `Option`/`Result` combinators such as `is_some_and`, `is_none_or`, `then_some`, `transpose`, and `inspect` when they keep ownership and control flow obvious; switch back to `match` once the closure logic stops being trivial.
- Prefer destructuring assignment, field init shorthand, and struct update syntax when they remove boilerplate without obscuring moves or borrow lifetimes.
- Prefer iterators/combinators over manual loops. Use `Cow<'_, str>` when allocation is conditional.
- Keep public API surfaces small. Use `#[must_use]` where return values matter.
- For a single SQLite database file, reuse the same store/connection pool across the process. Do not open a second independent connection/manager to the same DB for background writers or sidecar tasks; share the existing store instead.

## Workspace Dependency Management

All crates share a single source of truth for dependencies in the root `Cargo.toml`:

- **All dependencies must be declared in `[workspace.dependencies]`** at the repository root.
- Individual crates reference workspace dependencies using `{ workspace = true }` syntax.
- Path-based internal crates (e.g., `klaw-core`, `klaw-llm`) must also use `{ workspace = true }`.
- Optional/feature-gated dependencies use `{ workspace = true, optional = true }`.
- When adding features to a workspace dependency in a crate, use `{ workspace = true, features = [...] }`.

Example:

```toml
# Root Cargo.toml
[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["sync", "time", "macros", "rt"] }

# Sub-crate Cargo.toml
[dependencies]
serde = { workspace = true }
tokio = { workspace = true, features = ["fs"] }
```

## Coding Style & Naming Conventions

Follow Rust 2024 defaults and `rustfmt` output (4-space indentation, trailing commas where formatter adds them). Prefer:

- `snake_case` for modules/functions/files.
- `PascalCase` for types/traits.
- small modules with explicit ownership boundaries.

Use `thiserror` for error enums and avoid `unwrap()` in production paths.

When implementing tools in `klaw-tool`, make tool metadata LLM-friendly:

- Write `description` so model planners can clearly infer **when** to call the tool.
- Design `parameters` schema with strong guidance (clear field semantics, constraints/defaults, and practical examples) to improve call accuracy and argument quality.

## Testing Guidelines

Place unit tests next to implementation (`mod tests`), and integration tests under `*/tests/` (example: `klaw-core/tests/runtime_e2e.rs`).
Name tests by behavior, e.g., `validate_fails_when_active_provider_missing`. Add regression tests for bug fixes.

For tool and config changes, include enough test cases to cover core paths and edge cases (arg validation, provider/config routing, formatting, and error handling when applicable). Every modification should keep the relevant crate/workspace tests passing before completion.

## Config Persistence Safety

- Treat `~/.klaw/config.toml` as a shared source of truth across GUI panels and runtime helpers.
- **Never** persist config changes by mutating a stale in-memory `AppConfig` snapshot and writing the whole file back.
- When editing one config subsection, reload the latest on-disk config first, apply a targeted mutation, validate, then write the updated config.
- Prefer shared `ConfigStore` update APIs over panel-local `toml::to_string_pretty(...) + save_raw_toml(...)` flows so multiple editors do not clobber each other's saves.
- Add regression tests for cross-panel or cross-store save ordering whenever changing config persistence logic. At minimum, cover the case where two stale editors save different fields and both changes must survive.

## Documentation Guidelines (mdBook)

When adding or updating docs under `docs/src`, ensure they satisfy mdBook structure and rendering requirements:

- Every new page must be linked from `docs/src/SUMMARY.md` using a relative path.
- Use clear heading hierarchy (`#`, `##`, `###`) and stable section names.
- Prefer fenced code blocks with language tags for commands/config snippets.
- Use relative links for internal pages and full URLs for external references.
- Keep examples executable and consistent with current CLI/binary naming (for this repo: `klaw`).
- Validate documentation build with `mdbook build docs` when doc structure changes.

## Commit & Pull Request Guidelines

Recent history is short, but commit messages should be concise, imperative, and specific (e.g., `add config loading to cli commands`). Keep one logical change per commit.

PRs should include:

- purpose and impacted crates,
- test evidence (commands run + results),
- config/doc updates when behavior changes,
- sample CLI output when user-facing behavior is modified.

## Security & Configuration Tips

Never commit API keys. Prefer `env_key` in `~/.klaw/config.toml` (default expects `OPENAI_API_KEY`). If sharing configs, redact credentials.

## Module Documentation & Changelog

Each workspace crate must maintain its own documentation:

**CHANGELOG.md** (at crate root, e.g., `klaw-core/CHANGELOG.md`):

- Record main changes on every module modification
- Format with date and type: `Added` / `Changed` / `Fixed` / `Removed`

**README.md** (at crate root):

- Describe module capabilities, implementation, and architecture
- Keep in sync with code - update when descriptions become inaccurate

## GUI Layout Notes (egui)

- Use `StripBuilder` for major vertical regions (header/editor/footer) to keep sizing predictable.
- Let header/content text use natural height; avoid hard-coded header heights that create blank gaps.
- Keep editor width equal to parent container width (`available_width` + `add_sized`).
- When parent height is below the panel minimum height, enable one outer scroll area for the whole panel.
- Prefer global toast notifications (`egui-notify`) for operation feedback; avoid inline success/error blocks that shift layout.
- Do not block the egui render/update path on runtime or IO work. Avoid calling `recv_timeout`, network requests, disk-heavy syncs, or other waiting operations directly from render callbacks.
- For GUI-to-runtime communication, prefer a background request handle plus `try_recv`/polling from UI state. Reuse the non-blocking request pattern already used by MCP/Gateway/ACP-style panels instead of introducing new synchronous `request_`* calls in render code.
- Treat status refreshes as background work. Periodic polling (provider status, environment checks, gateway/runtime snapshots, etc.) must be scheduled asynchronously and coalesced so a slow prior refresh does not queue duplicate work.
- User-triggered actions that may take noticeable time should also avoid blocking the UI thread. Prefer pending task state, optimistic or staged UI updates, and completion toasts over synchronous waits.
- Keep the GUI runtime command loop responsive: slow commands should be spawned into tasks or subsystem workers instead of awaiting inline on the main command dispatcher.
- Do not silently drop GUI log delivery. If logs are buffered or lossy, track dropped message counts/bytes and surface that state in the Logs UI so "no new logs" and "logs were dropped" are distinguishable.
- GUI shutdown must be bounded. Do not rely on an unbounded `join()` or a single stalled runtime task to determine whether quit completes; use timeouts and warn when shutdown is incomplete.
- Add regression coverage for any GUI/runtime bridge change. At minimum, cover that slow background commands do not block status requests or quit, and that log-drop telemetry remains observable when the GUI falls behind.

## Git Commit Guidelines

Commit messages follow the [Conventional Commits](https://www.conventionalcommits.org/) specification. Each commit should be one logical change.

### Commit Message Format

```
<type>(<scope>): <subject>

<body>

<footer>
```

- **Subject line**: Required, imperative mood, lowercase, no trailing period, max 72 chars
- **Body**: Optional, explains *what* and *why*, not *how*
- **Footer**: Optional, use for `BREAKING CHANGE:`, `Closes #123`, etc.

### Commit Types


| Type       | Description                                 |
| ---------- | ------------------------------------------- |
| `feat`     | New feature                                 |
| `fix`      | Bug fix                                     |
| `docs`     | Documentation changes                       |
| `style`    | Code style (formatting, semicolons, etc.)   |
| `refactor` | Code refactoring without behavior change    |
| `perf`     | Performance improvements                    |
| `test`     | Test additions or corrections               |
| `chore`    | Maintenance tasks, dependencies, tooling    |
| `ci`       | CI/CD configuration changes                 |
| `build`    | Build system or external dependency changes |
| `revert`   | Reverting a previous commit                 |


### Examples

```
feat(cli): add agent mode for one-shot requests

Closes #42

feat(core): implement reliability retry with exponential backoff

Add retry policy with configurable max attempts and base delay.
Idempotency keys prevent duplicate processing on retry.

BREAKING CHANGE: AgentLoop now requires ReliabilityConfig parameter

fix(gui): resolve timestamp formatting in panel display

docs: add git commit guidelines to agents.md
```

### Pull Request Guidelines

PRs should include:

- Purpose and impacted crates
- Test evidence (commands run + results)
- Config/doc updates when behavior changes
- Sample CLI output when user-facing behavior is modified


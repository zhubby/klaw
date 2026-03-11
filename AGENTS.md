# Repository Guidelines

## Project Structure & Module Organization
This repository is a Rust workspace. Crates are split by responsibility:
- `klaw-core`: agent loop, protocol, reliability, scheduling.
- `klaw-cli`: CLI entrypoint and command handlers (`stdio`, `once`).
- `klaw-llm`: LLM provider integrations.
- `klaw-tool`: tool implementations and registry.
- `klaw-config`: TOML config loading/validation.
- `klaw-mcp`, `klaw-skill`, `klaw-memory`: MCP, skill, and memory support crates.
- `docs/`: mdBook sources (`docs/src`) and architecture notes (`docs/agent-core`).

Keep new code in the crate that owns the domain concern; avoid cross-crate leakage of CLI-specific logic into core runtime crates.

## Build, Test, and Development Commands
Use workspace-level Cargo commands from repo root:
- `cargo check --workspace`: fast compile verification.
- `cargo build --workspace`: build all crates.
- `cargo test --workspace`: run unit and integration tests.
- `cargo test -p klaw-core --test runtime_e2e`: run core E2E runtime tests.
- `cargo fmt --all`: apply Rust formatting.
- `cargo clippy --workspace --all-targets -- -D warnings`: lint strictly.
- `klaw stdio`: run interactive local agent loop.
- `klaw once --input "hello"`: single request/response run.

For docs: `mdbook build docs` (or `mdbook serve docs` for local preview).

## Coding Style & Naming Conventions
Follow Rust 2021 defaults and `rustfmt` output (4-space indentation, trailing commas where formatter adds them). Prefer:
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

## Commit & Pull Request Guidelines
Recent history is short, but commit messages should be concise, imperative, and specific (e.g., `add config loading to cli commands`). Keep one logical change per commit.

PRs should include:
- purpose and impacted crates,
- test evidence (commands run + results),
- config/doc updates when behavior changes,
- sample CLI output when user-facing behavior is modified.

## Security & Configuration Tips
Never commit API keys. Prefer `env_key` in `~/.klaw/config.toml` (default expects `OPENAI_API_KEY`). If sharing configs, redact credentials.

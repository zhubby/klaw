# klaw-util

`klaw-util` contains shared Klaw constants and filesystem path helpers for the default data directory layout.

It centralizes fixed names such as:

- `~/.klaw`
- `config.toml`
- `workspace/`
- `skills/`
- `skills-registry/`
- `tokenizers/`

Use this crate when a module needs to derive default local paths without pulling in higher-level storage or config abstractions.

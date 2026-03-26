# klaw-util

`klaw-util` contains shared Klaw constants, filesystem path helpers for the default data directory layout, and lightweight runtime environment helpers such as system timezone detection and external command PATH augmentation.

It centralizes fixed names such as:

- `~/.klaw`
- `config.toml`
- `workspace/`
- `skills/`
- `skills-registry/`
- `tokenizers/`

Use this crate when a module needs to derive default local paths or reuse host-level defaults like the detected system timezone without pulling in higher-level storage or config abstractions.

Current internal layout:

- `paths`: shared filesystem/data-dir constants and path derivation helpers
- `environment`: lightweight host/runtime environment types and helpers
- `command_path`: reusable external command PATH augmentation for GUI/macOS launches

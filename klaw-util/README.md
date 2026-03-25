# klaw-util

`klaw-util` contains shared Klaw constants, filesystem path helpers for the default data directory layout, and lightweight runtime environment helpers such as system timezone detection.

It centralizes fixed names such as:

- `~/.klaw`
- `config.toml`
- `workspace/`
- `skills/`
- `skills-registry/`
- `tokenizers/`

Use this crate when a module needs to derive default local paths or reuse host-level defaults like the detected system timezone without pulling in higher-level storage or config abstractions.

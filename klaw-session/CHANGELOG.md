# CHANGELOG

## 2026-03-16

### Added

- initial session manager crate with `SessionManager` trait, `SessionListQuery`, and `SqliteSessionManager`

### Changed

- expanded `SessionManager` to cover session state routing, chat history persistence, and provider/model updates so runtime and CLI can stop calling storage directly

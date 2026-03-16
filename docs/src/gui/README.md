# GUI Documentation

This directory contains documentation for the Klaw desktop GUI application.

## Contents

- [Architecture Overview](architecture.md) - Complete GUI architecture, module structure, and design patterns

## Quick Links

- [egui Documentation](https://docs.rs/egui)
- [eframe Documentation](https://docs.rs/eframe)
- [egui-phosphor](https://github.com/egui-phosphor/egui-phosphor)

## Overview

Klaw GUI is built with **egui**, an immediate-mode GUI framework in Rust. Key features:

- **Tabbed workbench** - Multi-panel workspace
- **State persistence** - Layout, theme, window size saved across sessions
- **15 feature panels** - Profile, Session, Provider, Memory, Skills, etc.
- **Toast notifications** - User feedback for operations
- **Theme support** - System/Light/Dark modes

## Module Structure

```
klaw-gui/src/
├── app/           # Main application (KlawGuiApp)
├── domain/        # Domain models (Menu)
├── ui/            # Shell, sidebar, workbench layout
├── panels/        # Feature panel implementations
├── state/         # UI state management & persistence
├── theme.rs       # Theme & fonts
├── notifications.rs
├── runtime_bridge.rs
└── widgets/
```

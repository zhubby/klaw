SHELL := /bin/bash

APP_NAME := Klaw
MACOS_TARGET := aarch64-apple-darwin
VERSION := $(shell awk -F' *= *' '/^version = / {gsub(/"/,"",$$2); print $$2; exit}' Cargo.toml)
DIST_DIR := dist/macos
APP_DIR := $(DIST_DIR)/$(APP_NAME).app
DMG_NAME := $(APP_NAME)-$(VERSION)-$(MACOS_TARGET).dmg
DMG_PATH := $(DIST_DIR)/$(DMG_NAME)

# klaw-webui → klaw-gateway 内嵌 `/chat`（输出目录见 .gitignore）
WASM_TARGET := wasm32-unknown-unknown
WEBUI_PKG_DIR := klaw-gateway/static/chat/pkg
WEBUI_WASM_RELEASE := target/$(WASM_TARGET)/release/klaw_webui.wasm
WASM_BINDGEN ?= wasm-bindgen
WASM_BINDGEN_VERSION := $(shell awk -F' *= *' '/^wasm-bindgen = / {gsub(/"/,"",$$2); print $$2; exit}' Cargo.toml)

.PHONY: build-macos-app package-macos-dmg clean-macos-artifacts webui-wasm clean-webui-wasm

build-macos-app:
	cargo build --release -p klaw-cli --target $(MACOS_TARGET)
	./scripts/macos/build_app.sh \
		--target $(MACOS_TARGET) \
		--version $(VERSION) \
		--output-dir $(DIST_DIR)

package-macos-dmg: build-macos-app
	./scripts/macos/package_dmg.sh \
		--app-path $(APP_DIR) \
		--output-path $(DMG_PATH)

clean-macos-artifacts:
	rm -rf $(DIST_DIR)

# 生成 `klaw-gateway/static/chat/pkg/`（需已安装与 workspace 对齐版本的 wasm-bindgen CLI）
webui-wasm:
	rustup target add $(WASM_TARGET)
	cargo build -p klaw-webui --target $(WASM_TARGET) --release
	@command -v $(WASM_BINDGEN) >/dev/null 2>&1 || { \
		echo "error: $(WASM_BINDGEN) not found; install e.g." >&2; \
		echo "  cargo install -f wasm-bindgen-cli --version $(WASM_BINDGEN_VERSION)" >&2; \
		exit 1; \
	}
	mkdir -p $(WEBUI_PKG_DIR)
	$(WASM_BINDGEN) $(WEBUI_WASM_RELEASE) \
		--out-dir $(WEBUI_PKG_DIR) --target web --no-typescript

clean-webui-wasm:
	rm -rf $(WEBUI_PKG_DIR)

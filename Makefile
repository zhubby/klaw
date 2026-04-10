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
WEBUI_DIST_DIR := klaw-gateway/static/chat/dist
WEBUI_WASM_RELEASE := target/$(WASM_TARGET)/release/klaw_webui.wasm
WASM_BINDGEN ?= wasm-bindgen
WASM_BINDGEN_VERSION := $(shell awk -F' *= *' '/^wasm-bindgen = / {gsub(/"/,"",$$2); print $$2; exit}' Cargo.toml)

.PHONY: build-macos-app package-macos-dmg clean-macos-artifacts webui-wasm clean-webui-wasm docs

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

# 生成 `klaw-gateway/static/chat/dist/`（需已安装与 workspace 对齐版本的 wasm-bindgen CLI）
webui-wasm:
	rustup target add $(WASM_TARGET)
	cargo build -p klaw-webui --target $(WASM_TARGET) --release
	@command -v $(WASM_BINDGEN) >/dev/null 2>&1 || { \
		echo "error: $(WASM_BINDGEN) not found; install e.g." >&2; \
		echo "  cargo install -f wasm-bindgen-cli --version $(WASM_BINDGEN_VERSION)" >&2; \
		exit 1; \
	}
	mkdir -p $(WEBUI_DIST_DIR)
	$(WASM_BINDGEN) $(WEBUI_WASM_RELEASE) \
		--out-dir $(WEBUI_DIST_DIR) --target web --no-typescript

clean-webui-wasm:
	rm -rf $(WEBUI_DIST_DIR)

REQUIRED_MDBOOK_VERSION := 0.4.40
REQUIRED_MDBOOK_MERMAID_VERSION := 0.14.0

docs:
	@echo "Checking mdbook version (required: $(REQUIRED_MDBOOK_VERSION))..."
	@if command -v mdbook >/dev/null 2>&1; then \
		INSTALLED_VERSION=$$(mdbook --version | awk '{print $$2}'); \
		if [ "$$INSTALLED_VERSION" != "$(REQUIRED_MDBOOK_VERSION)" ]; then \
			echo "mdbook version $$INSTALLED_VERSION found, expected $(REQUIRED_MDBOOK_VERSION)"; \
			echo "Installing correct version of mdbook..."; \
			cargo install mdbook --vers $(REQUIRED_MDBOOK_VERSION); \
		fi \
	else \
		echo "mdbook not found, installing version $(REQUIRED_MDBOOK_VERSION)..."; \
		cargo install mdbook --vers $(REQUIRED_MDBOOK_VERSION); \
	fi
	@echo "Checking mdbook-mermaid version (required: $(REQUIRED_MDBOOK_MERMAID_VERSION))..."
	@if command -v mdbook-mermaid >/dev/null 2>&1; then \
		INSTALLED_VERSION=$$(mdbook-mermaid --version | awk '{print $$2}'); \
		if [ "$$INSTALLED_VERSION" != "$(REQUIRED_MDBOOK_MERMAID_VERSION)" ]; then \
			echo "mdbook-mermaid version $$INSTALLED_VERSION found, expected $(REQUIRED_MDBOOK_MERMAID_VERSION)"; \
			echo "Installing correct version of mdbook-mermaid..."; \
			cargo install mdbook-mermaid --vers $(REQUIRED_MDBOOK_MERMAID_VERSION); \
		fi \
	else \
		echo "mdbook-mermaid not found, installing version $(REQUIRED_MDBOOK_MERMAID_VERSION)..."; \
		cargo install mdbook-mermaid --vers $(REQUIRED_MDBOOK_MERMAID_VERSION); \
	fi
	mdbook serve docs

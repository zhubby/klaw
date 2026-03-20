SHELL := /bin/bash

APP_NAME := Klaw
MACOS_TARGET := aarch64-apple-darwin
VERSION := $(shell awk -F' *= *' '/^version = / {gsub(/"/,"",$$2); print $$2; exit}' Cargo.toml)
DIST_DIR := dist/macos
APP_DIR := $(DIST_DIR)/$(APP_NAME).app
DMG_NAME := $(APP_NAME)-$(VERSION)-$(MACOS_TARGET).dmg
DMG_PATH := $(DIST_DIR)/$(DMG_NAME)

.PHONY: build-macos-app package-macos-dmg clean-macos-artifacts

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

SHELL        := /bin/bash
ROOT         := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
BUILD        := $(ROOT)/build
DIST         := $(BUILD)/target
CONFIG       ?= Release
INSTALL_DIR  ?= $(HOME)/.local/lib/deadlywp
BIN_DIR      ?= $(HOME)/.local/bin
APP_DIR      ?= $(HOME)/.local/share/applications
ICON_DIR     ?= $(HOME)/.local/share/icons/hicolor
BIN          := $(DIST)/$(if $(filter Release,$(CONFIG)),release,debug)/deadlywp
CARGO_FLAGS  := $(if $(filter Release,$(CONFIG)),--release,)
export CARGO_TARGET_DIR := $(DIST)

.PHONY: build check test run daemon ui install uninstall check-all release shaders icons clean

build:
	cargo build $(CARGO_FLAGS)

check:
	cargo check $(CARGO_FLAGS)

test:
	cargo test $(CARGO_FLAGS)

run: build
	$(BIN)

daemon: build
	$(BIN) daemon

ui: build
	$(BIN) ui

install: build
	install -d $(INSTALL_DIR) $(BIN_DIR) $(APP_DIR) $(ICON_DIR)/256x256/apps $(ICON_DIR)/32x32/apps
	install -m 755 $(BIN) $(INSTALL_DIR)/deadlywp
	ln -sfn $(INSTALL_DIR)/deadlywp $(BIN_DIR)/deadlywp
	install -m 644 $(ROOT)/assets/deadlywp.desktop $(APP_DIR)/deadlywp.desktop
	install -m 644 $(ROOT)/assets/icon.png $(ICON_DIR)/256x256/apps/deadlywp.png
	install -m 644 $(ROOT)/assets/icon-32.png $(ICON_DIR)/32x32/apps/deadlywp.png
	-update-desktop-database $(APP_DIR) 2>/dev/null
	-gtk-update-icon-cache -q $(ICON_DIR) 2>/dev/null

uninstall:
	rm -f $(BIN_DIR)/deadlywp $(APP_DIR)/deadlywp.desktop $(ICON_DIR)/256x256/apps/deadlywp.png $(ICON_DIR)/32x32/apps/deadlywp.png
	rm -rf $(INSTALL_DIR)
	rm -rf $(HOME)/.local/share/plasma/wallpapers/org.deadlywp.live

# Cross-check Apple targets with clang when building outside macOS.
APPLE_CC := $(if $(filter Darwin,$(shell uname -s)),,CC_aarch64_apple_darwin=clang CC_x86_64_apple_darwin=clang AR_aarch64_apple_darwin=ar AR_x86_64_apple_darwin=ar)
check-all:
	cargo check $(CARGO_FLAGS)
	cargo check $(CARGO_FLAGS) --target x86_64-pc-windows-msvc
	$(APPLE_CC) cargo check $(CARGO_FLAGS) --target x86_64-apple-darwin
	$(APPLE_CC) cargo check $(CARGO_FLAGS) --target aarch64-apple-darwin

release:
	bash $(ROOT)/scripts/release/pushReleaseTag.sh $(RELEASE_FLAGS)

# Requires qt6-shadertools.
shaders:
	qsb --qt6 --qsbversion 64 -o $(ROOT)/assets/plasma/contents/impl/adjust.frag.qsb $(ROOT)/assets/plasma/contents/impl/adjust.frag

# Requires ImageMagick.
icons:
	magick $(ROOT)/assets/logo.png -resize 256x256 $(ROOT)/assets/icon.png
	magick $(ROOT)/assets/logo.png -resize 32x32 $(ROOT)/assets/icon-32.png
	magick $(ROOT)/assets/logo.png -resize 64x64 $(ROOT)/assets/tray.png

clean:
	rm -rf $(DIST) $(BUILD) $(ROOT)/target

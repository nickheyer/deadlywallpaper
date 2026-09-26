# Deadly Wallpaper
SHELL        := /bin/bash
ROOT         := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
BUILD        := $(ROOT)/build
DIST         := $(BUILD)/target
CONFIG       ?= Release
INSTALL_DIR  ?= $(HOME)/.local/lib/deadlywp
BIN_DIR      ?= $(HOME)/.local/bin
BIN          := $(DIST)/$(if $(filter Release,$(CONFIG)),release,debug)/deadlywp
CARGO_FLAGS  := $(if $(filter Release,$(CONFIG)),--release,)
export CARGO_TARGET_DIR := $(DIST)

.PHONY: build check test run daemon ui install uninstall check-all clean

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
	install -d $(INSTALL_DIR) $(BIN_DIR)
	install -m 755 $(BIN) $(INSTALL_DIR)/deadlywp
	ln -sfn $(INSTALL_DIR)/deadlywp $(BIN_DIR)/deadlywp

uninstall:
	rm -f $(BIN_DIR)/deadlywp
	rm -rf $(INSTALL_DIR)

# Type-check every supported platform from one machine. Away from macOS the Apple targets
# use clang as a cross compiler for one header-free Objective-C helper.
APPLE_CC := $(if $(filter Darwin,$(shell uname -s)),,CC_aarch64_apple_darwin=clang CC_x86_64_apple_darwin=clang AR_aarch64_apple_darwin=ar AR_x86_64_apple_darwin=ar)
check-all:
	cargo check $(CARGO_FLAGS)
	cargo check $(CARGO_FLAGS) --target x86_64-pc-windows-msvc
	$(APPLE_CC) cargo check $(CARGO_FLAGS) --target x86_64-apple-darwin
	$(APPLE_CC) cargo check $(CARGO_FLAGS) --target aarch64-apple-darwin

clean:
	rm -rf $(DIST) $(BUILD) $(ROOT)/target

# Deadly Wallpaper
SHELL        := /bin/bash
ROOT         := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
BUILD        := $(ROOT)/build
DIST         := $(BUILD)/target
CONFIG       ?= Release
INSTALL_DIR  ?= $(HOME)/.local/lib/deadlywp
BIN_DIR      ?= $(HOME)/.local/bin

.PHONY: clean

clean:
	rm -rf $(DIST) $(BUILD)

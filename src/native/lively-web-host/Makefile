# lively-web-host: WebKitGTK HTML wallpaper renderer for the Lively Linux port.
#
#   make deps   fetch + build gtk-layer-shell v0.9.2 into $(PREFIX)
#   make        build $(BUILD_DIR)/lively-web-host
#   make test   run test/test.sh against a live Wayland session
#   make clean  remove $(BUILD_DIR)

BUILD_DIR ?= build
PREFIX    ?= $(abspath ../../../build/prefix)

GLS_TAG     ?= v0.9.2
GLS_TARBALL ?= https://github.com/wmww/gtk-layer-shell/archive/refs/tags/$(GLS_TAG).tar.gz
GLS_SRC     := $(BUILD_DIR)/deps/gtk-layer-shell
GLS_BUILD   := $(BUILD_DIR)/deps/gtk-layer-shell-build
GLS_PC      := $(PREFIX)/lib/pkgconfig/gtk-layer-shell-0.pc

PROTO_DIR := ../protocols
PROTO_XML := $(PROTO_DIR)/xdg-output-unstable-v1.xml
GEN_H     := $(BUILD_DIR)/xdg-output-unstable-v1-client-protocol.h
GEN_C     := $(BUILD_DIR)/xdg-output-unstable-v1-protocol.c

PKGS := gtk+-3.0 webkit2gtk-4.1 gtk-layer-shell-0 json-glib-1.0 gio-2.0 wayland-client
PKG_CONFIG_ENV := PKG_CONFIG_PATH=$(PREFIX)/lib/pkgconfig:$(PKG_CONFIG_PATH)
# Recursively expanded on purpose: gtk-layer-shell-0.pc only exists after `deps`.
PKG_CFLAGS = $(shell $(PKG_CONFIG_ENV) pkg-config --cflags $(PKGS))
PKG_LIBS   = $(shell $(PKG_CONFIG_ENV) pkg-config --libs $(PKGS))

ifeq ($(origin CC),default)
CC := gcc
endif
CFLAGS  ?= -O2
CFLAGS  += -std=c11 -Wall -Wextra -D_DEFAULT_SOURCE -I$(BUILD_DIR)
LDFLAGS += -Wl,-rpath,$(PREFIX)/lib -Wl,-rpath,'$$ORIGIN/lib'

SRCS := main.c args.c ipc.c stream_util.c props.c output_match.c
OBJS := $(patsubst %.c,$(BUILD_DIR)/obj/%.o,$(SRCS)) $(BUILD_DIR)/obj/xdg-output-unstable-v1-protocol.o
BIN  := $(BUILD_DIR)/lively-web-host

.PHONY: all deps clean test

all: deps $(BIN)

deps: $(GLS_PC)

$(GLS_PC):
	mkdir -p $(BUILD_DIR)/deps
	test -f $(GLS_SRC)/meson.build || { \
	  rm -rf $(GLS_SRC) $(GLS_SRC).tmp && mkdir -p $(GLS_SRC).tmp && \
	  curl -fsSL $(GLS_TARBALL) | tar -xz -C $(GLS_SRC).tmp --strip-components=1 && \
	  mv $(GLS_SRC).tmp $(GLS_SRC); }
	test -f $(GLS_BUILD)/build.ninja || meson setup $(GLS_BUILD) $(GLS_SRC) \
	  -Dexamples=false -Ddocs=false -Dtests=false -Dintrospection=false -Dvapi=false \
	  --prefix=$(PREFIX) --libdir=lib --buildtype=release
	ninja -C $(GLS_BUILD)
	ninja -C $(GLS_BUILD) install

$(GEN_H): $(PROTO_XML) | $(BUILD_DIR)
	wayland-scanner client-header $< $@

$(GEN_C): $(PROTO_XML) | $(BUILD_DIR)
	wayland-scanner private-code $< $@

$(BUILD_DIR) $(BUILD_DIR)/obj:
	mkdir -p $@

$(BUILD_DIR)/obj/xdg-output-unstable-v1-protocol.o: $(GEN_C) $(GEN_H) | $(BUILD_DIR)/obj
	$(CC) $(CFLAGS) $(PKG_CFLAGS) -c $< -o $@

$(BUILD_DIR)/obj/%.o: %.c $(wildcard *.h) $(GEN_H) | $(BUILD_DIR)/obj $(GLS_PC)
	$(CC) $(CFLAGS) $(PKG_CFLAGS) -c $< -o $@

$(BIN): $(OBJS) | $(GLS_PC)
	$(CC) $(CFLAGS) -o $@ $(OBJS) $(LDFLAGS) $(PKG_LIBS)

UNIT := $(BUILD_DIR)/stream_util_test

$(UNIT): test/stream_util_test.c $(BUILD_DIR)/obj/stream_util.o | $(BUILD_DIR)
	$(CC) $(CFLAGS) $(PKG_CFLAGS) -o $@ test/stream_util_test.c $(BUILD_DIR)/obj/stream_util.o $(LDFLAGS) $(PKG_LIBS)

test: all $(UNIT)
	$(UNIT)
	BUILD_DIR=$(abspath $(BUILD_DIR)) ./test/test.sh $(abspath $(BIN))

clean:
	rm -rf $(BUILD_DIR)

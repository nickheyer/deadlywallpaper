# lively-wl-monitor: Wayland output and toplevel reporter for the Lively Linux core.
#
# make [BUILD_DIR=build] [PREFIX=/usr/local]  builds $(BUILD_DIR)/lively-wl-monitor
# make test                                    runs the --once self check (see README.md)
# make register-kde / unregister-kde           KWin only: allow this binary to receive
#                                              org_kde_plasma_window_management

BUILD_DIR ?= build
PREFIX ?= /usr/local

CC ?= cc
PKG_CONFIG ?= pkg-config
WAYLAND_SCANNER ?= wayland-scanner
PROTOCOL_DIR ?= ../protocols
PYTHON ?= python3

WAYLAND_CFLAGS := $(shell $(PKG_CONFIG) --cflags wayland-client)
WAYLAND_LIBS := $(shell $(PKG_CONFIG) --libs wayland-client)

ALL_CFLAGS := -std=c11 -Wall -Wextra -O2 $(CFLAGS) $(WAYLAND_CFLAGS) -I$(BUILD_DIR)
ALL_LDLIBS := $(WAYLAND_LIBS) $(LDLIBS)

PROTOCOLS := xdg-output-unstable-v1 \
             wlr-foreign-toplevel-management-unstable-v1 \
             plasma-window-management \
             plasma-virtual-desktop
PROTOCOL_HEADERS := $(patsubst %,$(BUILD_DIR)/%-client-protocol.h,$(PROTOCOLS))
PROTOCOL_SOURCES := $(patsubst %,$(BUILD_DIR)/%-protocol.c,$(PROTOCOLS))
OBJECTS := $(BUILD_DIR)/main.o $(PROTOCOL_SOURCES:.c=.o)
BINARY := $(BUILD_DIR)/lively-wl-monitor

.PHONY: all clean test register-kde unregister-kde
.SECONDARY:

all: $(BINARY)

$(BUILD_DIR):
	mkdir -p $@

$(BUILD_DIR)/%-client-protocol.h: $(PROTOCOL_DIR)/%.xml | $(BUILD_DIR)
	$(WAYLAND_SCANNER) client-header $< $@

$(BUILD_DIR)/%-protocol.c: $(PROTOCOL_DIR)/%.xml | $(BUILD_DIR)
	$(WAYLAND_SCANNER) private-code $< $@

$(BUILD_DIR)/%-protocol.o: $(BUILD_DIR)/%-protocol.c | $(BUILD_DIR)
	$(CC) $(ALL_CFLAGS) -c -o $@ $<

$(BUILD_DIR)/main.o: main.c $(PROTOCOL_HEADERS) | $(BUILD_DIR)
	$(CC) $(ALL_CFLAGS) -c -o $@ $<

$(BINARY): $(OBJECTS)
	$(CC) $(ALL_CFLAGS) $(LDFLAGS) -o $@ $(OBJECTS) $(ALL_LDLIBS)

clean:
	rm -rf $(BUILD_DIR)

# Runs one --once pass, checks that every line is JSON, then checks the
# content against the outputs of the development session (test_once.py).
test: $(BINARY)
	$(BINARY) --once > $(BUILD_DIR)/once.jsonl
	$(PYTHON) -c 'import json,sys; [json.loads(l) for l in sys.stdin]' < $(BUILD_DIR)/once.jsonl
	$(PYTHON) test_once.py < $(BUILD_DIR)/once.jsonl

# KWin only exposes org_kde_plasma_window_management to executables registered
# through a .desktop file; kde-register.sh writes the same file the core writes
# for the binary in $(BUILD_DIR) (named after the binary path, so builds and
# installs at different paths coexist), rebuilds the service cache and waits
# until KWin exposes the interface.
register-kde: $(BINARY)
	sh kde-register.sh register $(BINARY)

unregister-kde:
	sh kde-register.sh unregister $(BINARY)

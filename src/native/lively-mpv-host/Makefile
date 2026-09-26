# lively-mpv-host: libmpv + wlr-layer-shell wallpaper renderer for the Lively Linux port.
BUILD_DIR ?= build
PREFIX ?= /usr/local
ifeq ($(origin CC),default)
CC := gcc
endif

PKGS := wayland-client wayland-egl egl glesv2 mpv
PKG_CFLAGS := $(shell pkg-config --cflags $(PKGS))
PKG_LIBS := $(shell pkg-config --libs $(PKGS))

PROTOCOL_DIR := ../protocols
THIRD_PARTY_DIR := ../third_party
PROTOCOLS := wlr-layer-shell-unstable-v1 xdg-output-unstable-v1 xdg-shell

CFLAGS ?= -Wall -Wextra -O2
CFLAGS += -std=c11 -D_GNU_SOURCE $(PKG_CFLAGS) -I$(BUILD_DIR) -I$(THIRD_PARTY_DIR)
LDLIBS := $(PKG_LIBS) -lm

TARGET := $(BUILD_DIR)/lively-mpv-host
SRCS := main.c wl.c player.c ipc.c
OBJS := $(addprefix $(BUILD_DIR)/,$(SRCS:.c=.o))
PROTO_HDRS := $(addprefix $(BUILD_DIR)/,$(addsuffix -client-protocol.h,$(PROTOCOLS)))
PROTO_SRCS := $(addprefix $(BUILD_DIR)/,$(addsuffix -protocol.c,$(PROTOCOLS)))
PROTO_OBJS := $(PROTO_SRCS:.c=.o)
CJSON_OBJ := $(BUILD_DIR)/cJSON.o

.PHONY: all clean test
.SECONDARY: $(PROTO_SRCS)

all: $(TARGET)

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

$(BUILD_DIR)/%-client-protocol.h: $(PROTOCOL_DIR)/%.xml | $(BUILD_DIR)
	wayland-scanner client-header $< $@

$(BUILD_DIR)/%-protocol.c: $(PROTOCOL_DIR)/%.xml | $(BUILD_DIR)
	wayland-scanner private-code $< $@

$(BUILD_DIR)/%-protocol.o: $(BUILD_DIR)/%-protocol.c $(PROTO_HDRS)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/%.o: %.c app.h $(PROTO_HDRS) | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(CJSON_OBJ): $(THIRD_PARTY_DIR)/cJSON.c $(THIRD_PARTY_DIR)/cJSON.h | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(TARGET): $(OBJS) $(PROTO_OBJS) $(CJSON_OBJ)
	$(CC) $(CFLAGS) $^ -o $@ $(LDLIBS)

clean:
	rm -rf $(BUILD_DIR)

test: $(TARGET)
	BUILD_DIR=$(BUILD_DIR) ./test.sh

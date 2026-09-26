# Deadly Wallpaper — Linux build of Lively Wallpaper
#
# Everything you need to build, run, test and install the Linux port lives here.
# Run `make help` for the list of targets.

SHELL        := /bin/bash
.DEFAULT_GOAL := help

ROOT         := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
BUILD        := $(ROOT)/build
PREFIX       := $(BUILD)/prefix
NATIVE_BUILD := $(BUILD)/native
DIST         := $(BUILD)/dist
CONFIG       ?= Release
INSTALL_DIR  ?= $(HOME)/.local/lib/lively-wallpaper
BIN_DIR      ?= $(HOME)/.local/bin

SLN_DIR      := $(ROOT)/src/Lively
CORE_PROJ    := $(SLN_DIR)/Lively.Core.Linux/Lively.Core.Linux.csproj
UI_PROJ      := $(SLN_DIR)/Lively.UI.Avalonia/Lively.UI.Avalonia.csproj
CLI_PROJ     := $(SLN_DIR)/Lively.Utility.Commandline/Lively.Utility.Commandline.csproj
WPF_PROJ     := $(SLN_DIR)/Lively/Lively.csproj

NATIVE_DIRS  := lively-wl-monitor lively-mpv-host lively-web-host
NATIVE_BINS  := $(addprefix $(NATIVE_BUILD)/,$(NATIVE_DIRS))
PLASMA_PKG   := $(ROOT)/src/plasma/com.lively.wallpaper
PLASMA_DEST  := $(HOME)/.local/share/plasma/wallpapers/com.lively.wallpaper

DOTNET       ?= dotnet
DOTNET_FLAGS := --nologo -c $(CONFIG)

# Arch Linux package names for everything the Linux port needs at build and run time.
PACMAN_PKGS  := dotnet-sdk mpv gtk3 webkit2gtk-4.1 json-glib wayland wayland-protocols \
                mesa libglvnd gcc make meson ninja pkgconf git ffmpeg libpulse \
                qt6-multimedia-ffmpeg qt6-webengine qt6-websockets qt6-shadertools

.PHONY: help deps native core ui cli all run run-core run-ui stop test test-native test-dotnet \
        windows-check plasma-install plasma-uninstall install uninstall clean distclean logs

help: ## Show this help
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage: make \033[36m<target>\033[0m\n\n"} /^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) }' $(MAKEFILE_LIST)
	@echo
	@echo "Build output: $(BUILD)"

##@ Setup

deps: ## Install system packages with pacman (Arch Linux; asks for sudo)
	sudo pacman -S --needed $(PACMAN_PKGS)

##@ Build

native: $(NATIVE_BINS) ## Build the native Wayland helpers (lively-wl-monitor, lively-mpv-host, lively-web-host)

$(NATIVE_BUILD)/%: FORCE
	$(MAKE) -C $(ROOT)/src/native/$* BUILD_DIR=$(NATIVE_BUILD)/$*.build PREFIX=$(PREFIX) all
	@mkdir -p $(NATIVE_BUILD)
	cp $(NATIVE_BUILD)/$*.build/$* $@

FORCE:

core: ## Build the Linux core daemon (Lively.Core.Linux)
	$(DOTNET) build $(CORE_PROJ) $(DOTNET_FLAGS)

ui: ## Build the Avalonia desktop UI (Lively.UI.Avalonia)
	$(DOTNET) build $(UI_PROJ) $(DOTNET_FLAGS)

cli: ## Build the command line client (livelycu)
	$(DOTNET) build $(CLI_PROJ) $(DOTNET_FLAGS)

all: native core ui cli dist ## Build everything and assemble build/dist

dist: ## Assemble a runnable tree in build/dist (core + UI + CLI + native helpers + Plasma plugin)
	rm -rf $(DIST)
	$(DOTNET) publish $(CORE_PROJ) $(DOTNET_FLAGS) -o $(DIST)/core
	$(DOTNET) publish $(UI_PROJ)   $(DOTNET_FLAGS) -o $(DIST)/core/plugins/UI
	$(DOTNET) publish $(CLI_PROJ)  $(DOTNET_FLAGS) -o $(DIST)/cli
	mkdir -p $(DIST)/core/plugins/native $(DIST)/core/plugins/plasma $(DIST)/core/plugins/mpv
	cp $(NATIVE_BINS) $(DIST)/core/plugins/native/
	cp -r $(PREFIX)/lib $(DIST)/core/plugins/native/lib
	cp -r $(PLASMA_PKG) $(DIST)/core/plugins/plasma/
	cp $(SLN_DIR)/Lively/Assets/Plugins/Mpv/LivelyProperties*.json $(DIST)/core/plugins/mpv/
	cp $(ROOT)/src/native/scripts/lively-core.sh $(DIST)/lively-core
	cp $(ROOT)/src/native/scripts/lively-ui.sh   $(DIST)/lively-ui
	cp $(ROOT)/src/native/scripts/livelycu.sh    $(DIST)/livelycu
	chmod +x $(DIST)/lively-core $(DIST)/lively-ui $(DIST)/livelycu
	@echo "dist ready: $(DIST)"

##@ Run
dev: run

run: dist ## Run the core (which launches the UI) from build/dist in the foreground
	$(DIST)/lively-core

run-core: core native ## Run the core daemon straight from the build tree (developer mode)
	LIVELY_NATIVE_DIR=$(NATIVE_BUILD) LIVELY_PLASMA_PKG=$(PLASMA_PKG) LIVELY_UI_COMMAND="$(DOTNET) run --project $(UI_PROJ) -c $(CONFIG) --no-build --" \
	  $(DOTNET) run --project $(CORE_PROJ) -c $(CONFIG) --no-build

run-ui: ui ## Run the desktop UI straight from the build tree (core must be running)
	$(DOTNET) run --project $(UI_PROJ) -c $(CONFIG) --no-build -- --showApp true

stop: ## Ask a running core to shut down (via the CLI)
	$(DOTNET) run --project $(CLI_PROJ) -c $(CONFIG) --no-build -- --shutdown true

logs: ## Tail the core and UI logs
	tail -n 50 -F "$$HOME/.local/share/Lively Wallpaper/logs/"*.txt "$$HOME/.local/share/Lively Wallpaper/UI/"*.txt

##@ Test

test: test-native test-dotnet ## Run every test suite

test-native: native ## Run the native helper self-tests (they briefly show test surfaces on your smallest display; override with LIVELY_TEST_OUTPUT=NAME)
	@LIVELY_TEST_OUTPUT="$${LIVELY_TEST_OUTPUT:-$$($(ROOT)/src/native/scripts/test-output.sh $(NATIVE_BUILD)/lively-wl-monitor)}"; \
	[ -n "$$LIVELY_TEST_OUTPUT" ] || { echo "could not determine a test output; set LIVELY_TEST_OUTPUT"; exit 1; }; \
	export LIVELY_TEST_OUTPUT; echo "native tests draw on output $$LIVELY_TEST_OUTPUT"; \
	case ":$$XDG_CURRENT_DESKTOP:" in *[Kk][Dd][Ee]*) \
	  $(MAKE) -C $(ROOT)/src/native/lively-wl-monitor BUILD_DIR=$(NATIVE_BUILD)/lively-wl-monitor.build PREFIX=$(PREFIX) register-kde || exit 1;; \
	esac; \
	for d in $(NATIVE_DIRS); do \
	  echo "== $$d"; $(MAKE) -C $(ROOT)/src/native/$$d BUILD_DIR=$(NATIVE_BUILD)/$$d.build PREFIX=$(PREFIX) test || exit 1; \
	done
	$(ROOT)/src/plasma/test/run_tests.sh

TEST_PROJS := Lively.Core.Linux.Tests Lively.Common.Linux.Tests Lively.Common.Linux.Feeds.Tests

test-dotnet: ## Run the .NET unit tests
	@for p in $(TEST_PROJS); do \
	  echo "== $$p"; $(DOTNET) test $(SLN_DIR)/$$p/$$p.csproj $(DOTNET_FLAGS) || exit 1; \
	done

windows-check: ## Cross-compile the Windows core on Linux to prove the shared refactor still builds for Windows
	$(DOTNET) build $(WPF_PROJ) $(DOTNET_FLAGS) -p:EnableWindowsTargeting=true

##@ Install

plasma-install: ## Install the Plasma wallpaper plugin into ~/.local/share/plasma/wallpapers
	rm -rf $(PLASMA_DEST)
	mkdir -p $(dir $(PLASMA_DEST))
	cp -r $(PLASMA_PKG) $(PLASMA_DEST)

plasma-uninstall: ## Remove the Plasma wallpaper plugin
	rm -rf $(PLASMA_DEST)

install: dist plasma-install ## Install to ~/.local (binaries in ~/.local/bin, app in ~/.local/lib/lively-wallpaper)
	rm -rf $(INSTALL_DIR)
	mkdir -p $(INSTALL_DIR) $(BIN_DIR) $(HOME)/.local/share/applications $(HOME)/.local/share/icons/hicolor/96x96/apps
	cp -r $(DIST)/. $(INSTALL_DIR)/
	ln -sf $(INSTALL_DIR)/lively-core $(BIN_DIR)/lively-core
	ln -sf $(INSTALL_DIR)/lively-ui   $(BIN_DIR)/lively-ui
	ln -sf $(INSTALL_DIR)/livelycu    $(BIN_DIR)/livelycu
	sed "s#@INSTALL_DIR@#$(INSTALL_DIR)#g" $(ROOT)/src/native/scripts/lively-wallpaper.desktop > $(HOME)/.local/share/applications/lively-wallpaper.desktop
	cp $(SLN_DIR)/Lively/Resources/appicon_96.png $(HOME)/.local/share/icons/hicolor/96x96/apps/lively-wallpaper.png
	@echo "Installed. Start with: lively-core   (or from your application menu: Lively Wallpaper)"

uninstall: plasma-uninstall ## Remove the installed files from ~/.local
	rm -rf $(INSTALL_DIR)
	rm -f $(BIN_DIR)/lively-core $(BIN_DIR)/lively-ui $(BIN_DIR)/livelycu
	rm -f $(HOME)/.local/share/applications/lively-wallpaper.desktop
	rm -f $(HOME)/.local/share/icons/hicolor/96x96/apps/lively-wallpaper.png

##@ Clean

clean: ## Remove build outputs (keeps downloaded native dependencies)
	rm -rf $(NATIVE_BUILD) $(DIST)
	find $(SLN_DIR) -type d \( -name bin -o -name obj \) -prune -exec rm -rf {} +

distclean: clean ## Remove everything under build/, including vendored native dependencies
	rm -rf $(BUILD)

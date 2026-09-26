#!/bin/sh
# Registers or unregisters lively-wl-monitor with KWin.
#
# KWin only exposes org_kde_plasma_window_management to executables whose
# .desktop file (Exec= the canonical binary path) lists the interface in
# X-KDE-Wayland-Interfaces. KWin looks the file up through the ksycoca service
# cache, whose file name depends on XDG_DATA_DIRS/XDG_DATA_HOME, so the cache
# is rebuilt with the values of the session that started KWin (the systemd
# user manager launches plasma-kwin_wayland.service).
#
# The desktop file is named after the binary path
# (lively-wl-monitor-<first 8 hex digits of sha1(path)>.desktop) and its
# content is byte for byte what the core writes (KdeInterfaceRegistration.cs),
# so a build directory, build/dist and an install can all be registered at the
# same time and the core never rewrites a file this script wrote.
#
# usage: kde-register.sh register BINARY
#        kde-register.sh unregister BINARY
set -eu

data_home="${XDG_DATA_HOME:-$HOME/.local/share}"

desktop_file_for() {
    hash=$(printf '%s' "$1" | sha1sum | cut -c1-8)
    printf '%s/applications/lively-wl-monitor-%s.desktop\n' "$data_home" "$hash"
}

desktop_file_content() {
    printf '[Desktop Entry]\n'
    printf 'Type=Application\n'
    printf 'Name=Lively Wallpaper window monitor\n'
    printf 'Comment=Lets Lively Wallpaper see which windows are open so it can pause wallpapers under fullscreen apps\n'
    printf 'Exec=%s\n' "$1"
    printf 'NoDisplay=true\n'
    printf 'Terminal=false\n'
    printf 'X-KDE-Wayland-Interfaces=org_kde_plasma_window_management,org_kde_plasma_virtual_desktop_management\n'
}

rebuild_service_cache() {
    if command -v systemctl >/dev/null 2>&1 && session_env=$(systemctl --user show-environment 2>/dev/null); then
        for var in XDG_DATA_DIRS XDG_DATA_HOME; do
            value=$(printf '%s\n' "$session_env" | sed -n "s/^$var=//p")
            if [ -n "$value" ]; then
                export "$var=$value"
            else
                unset "$var"
            fi
        done
    fi
    kbuildsycoca6
}

case "${1:-}" in
register)
    binary=$(realpath "$2")
    desktop_file=$(desktop_file_for "$binary")
    mkdir -p "$(dirname "$desktop_file")"
    desktop_file_content "$binary" > "$desktop_file"
    rebuild_service_cache
    # KWin reloads the cache asynchronously; wait until it exposes the interface.
    probe=$(mktemp)
    trap 'rm -f "$probe"' EXIT
    attempt=0
    while [ "$attempt" -lt 20 ]; do
        "$binary" --once > "$probe" 2>/dev/null || true
        if head -n 1 "$probe" | grep -q '"toplevel_protocol":"plasma"'; then
            echo "registered $desktop_file: KWin exposes org_kde_plasma_window_management to $binary"
            exit 0
        fi
        attempt=$((attempt + 1))
        sleep 0.5
    done
    echo "KWin still does not expose org_kde_plasma_window_management to $binary after registering $desktop_file" >&2
    exit 1
    ;;
unregister)
    binary=$(realpath "$2")
    desktop_file=$(desktop_file_for "$binary")
    rm -f "$desktop_file"
    rebuild_service_cache
    echo "removed $desktop_file"
    ;;
*)
    echo "usage: $0 register BINARY | unregister BINARY" >&2
    exit 2
    ;;
esac

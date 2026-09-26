#!/usr/bin/env bash
# Launches the Lively core daemon from the directory this script lives in.
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
export LIVELY_NATIVE_DIR="${LIVELY_NATIVE_DIR:-$HERE/core/plugins/native}"
export LIVELY_PLASMA_PKG="${LIVELY_PLASMA_PKG:-$HERE/core/plugins/plasma/com.lively.wallpaper}"
export LIVELY_UI_COMMAND="${LIVELY_UI_COMMAND:-$HERE/lively-ui}"
exec "$HERE/core/Lively.Core.Linux" "$@"

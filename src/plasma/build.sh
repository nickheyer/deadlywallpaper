#!/usr/bin/env bash
# Compiles the AdjustLayer fragment shader to the Qt shader pack the plugin loads at runtime.
# Requires qt6-shadertools (/usr/lib/qt6/bin/qsb).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
QSB="${QSB:-/usr/lib/qt6/bin/qsb}"
SHADER_DIR="$HERE/com.lively.wallpaper/contents/shaders"

if [ ! -x "$QSB" ]; then
    echo "qsb not found at $QSB (install qt6-shadertools)" >&2
    exit 1
fi

"$QSB" --glsl "100 es,120,150" --hlsl 50 --msl 12 -o "$SHADER_DIR/adjust.frag.qsb" "$SHADER_DIR/adjust.frag"
echo "wrote $SHADER_DIR/adjust.frag.qsb"

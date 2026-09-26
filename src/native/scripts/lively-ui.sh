#!/usr/bin/env bash
# Launches the Lively desktop UI. Normally the core starts it; running it by hand also works when the core is up.
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
exec "$HERE/core/plugins/UI/Lively.UI.Avalonia" "$@"

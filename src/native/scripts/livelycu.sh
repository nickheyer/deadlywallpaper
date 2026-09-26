#!/usr/bin/env bash
# Lively command line client: livelycu --help
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
exec "$HERE/cli/Lively.Utility.Commandline" "$@"

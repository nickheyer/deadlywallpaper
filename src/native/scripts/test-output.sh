#!/usr/bin/env bash
# Prints the Wayland output name the native self-tests should draw on.
#
# Honors LIVELY_TEST_OUTPUT when set; otherwise asks lively-wl-monitor (first
# argument, or the sibling build) for the outputs and picks the smallest one so
# the tests disturb the desktop as little as possible.
set -eu
if [ -n "${LIVELY_TEST_OUTPUT:-}" ]; then
    printf '%s\n' "$LIVELY_TEST_OUTPUT"
    exit 0
fi
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
MONITOR="${1:-$HERE/../lively-wl-monitor/build/lively-wl-monitor}"
if [ ! -x "$MONITOR" ]; then
    echo "test-output.sh: $MONITOR is not built; run make in src/native/lively-wl-monitor or set LIVELY_TEST_OUTPUT" >&2
    exit 1
fi
"$MONITOR" --once | python3 -c '
import json, sys
outputs = []
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    event = json.loads(line)
    if event.get("event") == "outputs":
        outputs = event["outputs"]
if not outputs:
    sys.exit("test-output.sh: lively-wl-monitor reported no outputs")
smallest = min(outputs, key=lambda o: o["width"] * o["height"])
print(smallest["name"])
'

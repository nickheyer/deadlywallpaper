#!/usr/bin/env bash
# Integration test for lively-web-host, run by `make test`.
# Needs a live Wayland session (layer-shell + xdg-output) with an output named
# $LIVELY_TEST_OUTPUT (default DP-3), and python3 with Pillow for the PNG check.
set -euo pipefail

HOST=${1:?usage: test.sh /path/to/lively-web-host}
: "${BUILD_DIR:?BUILD_DIR must point at the build directory}"
OUTPUT=${LIVELY_TEST_OUTPUT:-DP-3}
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SHOT="$BUILD_DIR/shot.png"
OUT="$BUILD_DIR/test-stdout.log"
ERR="$BUILD_DIR/test-stderr.log"

rm -f "$SHOT" "$OUT" "$ERR"
: > "$OUT"

failures=0
fail() { echo "FAIL: $*" >&2; failures=$((failures + 1)); }
pass() { echo "ok:   $*"; }

wait_for() { # wait_for <grep pattern> <seconds>
    local pattern=$1 ticks=$(( $2 * 10 ))
    while (( ticks-- > 0 )); do
        grep -q -- "$pattern" "$OUT" && return 0
        sleep 0.1
    done
    echo "timeout waiting for $pattern" >&2
    return 1
}

feed() {
    wait_for '"Type":2' 30 || return 1
    echo '{"Type":9,"Volume":50}'
    echo '{"Type":12,"Name":"speed","Value":30.0,"Step":1.0}'
    echo '{"Type":18,"Name":"showGrid","Value":false}'
    echo '{"Type":17,"Name":"accent","Value":"#00ccff"}'
    echo '{"Type":13,"Name":"caption","Value":"from stdin"}'
    echo '{"Type":14,"Name":"shape","Value":2}'
    echo '{"Type":15,"Name":"asset","Value":"assets/sample.txt"}'
    echo '{"Type":15,"Name":"asset","Value":"assets/missing.txt"}'
    echo '{"Type":10,"Info":{"NameCpu":"test-cpu","CurrentCpu":12.5}}'
    echo '{"Type":11,"Info":{"Title":"test-track","Artist":"test-artist"}}'
    echo '{"Type":20,"Data":[0.5,0.25]}'
    echo '{"Type":16,"Name":"resetButton","IsDefault":false}'
    sleep 0.5
    echo '{"Type":7}'
    sleep 2
    echo '{"Type":8}'
    sleep 1
    echo "{\"Type\":6,\"Format\":1,\"FilePath\":\"$SHOT\",\"Delay\":0}"
    wait_for '"Type":3' 15 || return 1
    echo '{"Type":16,"Name":"resetButton","IsDefault":true}'
    sleep 0.3
    echo '{"Type":5}'
}

echo "running $HOST on output $OUTPUT"
set +e
feed | "$HOST" --output "$OUTPUT" --verbose --volume 0 --pause-event --audio \
    --property "$HERE/LivelyProperties.json" --type local "$HERE/index.html" > "$OUT" 2> "$ERR"
code=${PIPESTATUS[1]}
set -e

echo "--- host stdout"
cat "$OUT"
echo "--- host exit code: $code"

[[ $code -eq 0 ]] && pass "exit code 0 after cmd_close" || fail "exit code $code, expected 0"

grep -q '"Type":0' "$OUT" && pass "msg_hwnd" || fail "no msg_hwnd (Type 0)"
grep -q '"Type":2,"Success":true' "$OUT" && pass "msg_wploaded Success:true" || fail "no msg_wploaded with Success:true"
grep '"Type":1' "$OUT" | grep -q 'lively-web-host test page loaded' \
    && pass "page console.log forwarded as msg_console" || fail "page console.log not forwarded"
grep -q '"Type":3,"FileName":"shot.png","Success":true' "$OUT" \
    && pass "msg_screenshot Success:true" || fail "no msg_screenshot with Success:true"

# LivelyProperties.json applied after load (values from the file)
grep -q 'prop speed=10' "$OUT" && pass "slider default applied from LivelyProperties.json" || fail "slider default not applied"
grep -q 'prop showGrid=true' "$OUT" && pass "checkbox default applied" || fail "checkbox default not applied"
grep -q 'prop accent=\\"#ff8800\\"' "$OUT" && pass "color picker default applied" || fail "color default not applied"
grep -q 'prop caption=\\"lively-web-host\\"' "$OUT" && pass "textbox default applied" || fail "textbox default not applied"
grep -q 'prop shape=1' "$OUT" && pass "dropdown default applied" || fail "dropdown default not applied"
grep -q 'prop asset=\\"assets/sample.txt\\"' "$OUT" && pass "folderDropdown resolved relative to the page" || fail "folderDropdown default not applied"
sed '/"Type":2,"Success":true/q' "$OUT" | grep -q 'prop resetButton' \
    && fail "button applied from LivelyProperties.json (must be skipped)" || pass "button skipped when restoring properties"

# lp_* messages from stdin
grep -q 'prop speed=30' "$OUT" && pass "lp_slider" || fail "lp_slider not delivered"
grep -q 'prop showGrid=false' "$OUT" && pass "lp_chekbox" || fail "lp_chekbox not delivered"
grep -q 'prop accent=\\"#00ccff\\"' "$OUT" && pass "lp_cpicker" || fail "lp_cpicker not delivered"
grep -q 'prop caption=\\"from stdin\\"' "$OUT" && pass "lp_textbox" || fail "lp_textbox not delivered"
grep -q 'prop shape=2' "$OUT" && pass "lp_dropdown" || fail "lp_dropdown not delivered"
grep -q 'prop asset=null' "$OUT" && pass "lp_fdropdown passes null for a missing file" || fail "lp_fdropdown missing-file null not delivered"
grep -q 'prop resetButton=true' "$OUT" && pass "lp_button" || fail "lp_button not delivered"
grep -q 'sysinfo {\\"NameCpu\\":\\"test-cpu\\"' "$OUT" && pass "lsp_perfcntr" || fail "lsp_perfcntr not delivered"
grep -q 'track {\\"Title\\":\\"test-track\\"' "$OUT" && pass "lsp_nowplaying" || fail "lsp_nowplaying not delivered"
grep -q 'audio first frame bands=2' "$OUT" && pass "lsp_audio" || fail "lsp_audio not delivered"

# pause/resume: the view is hidden while paused and the page sees it
grep -q 'playback {\\"IsPaused\\":true}' "$OUT" && pass "playback changed event on suspend" || fail "no playback event on suspend"
grep -q 'visibility=hidden' "$OUT" && pass "page hidden while paused" || fail "page not hidden on cmd_suspend"
grep -q 'playback {\\"IsPaused\\":false}' "$OUT" && pass "playback changed event on resume" || fail "no playback event on resume"
grep -q 'visibility=visible' "$OUT" && pass "page visible again after resume" || fail "page not visible after cmd_resume"

if [[ -f "$SHOT" ]]; then
    if python3 - "$SHOT" <<'PY'
import sys
from PIL import Image, ImageStat
im = Image.open(sys.argv[1]).convert("RGB")
st = ImageStat.Stat(im)
mean = sum(st.mean) / 3
stddev = sum(st.stddev) / 3
print(f"screenshot {im.size[0]}x{im.size[1]} mean={mean:.1f} stddev={stddev:.1f}")
ok = im.size[0] >= 100 and im.size[1] >= 100 and 5 < mean < 250 and stddev > 5
sys.exit(0 if ok else 1)
PY
    then pass "screenshot PNG has non-trivial content"; else fail "screenshot PNG is empty or uniform"; fi
else
    fail "screenshot $SHOT was not written"
fi

if (( failures > 0 )); then
    echo "--- host stderr (last 40 lines)"
    tail -40 "$ERR"
    echo "$failures check(s) failed"
    exit 1
fi
echo "all checks passed"

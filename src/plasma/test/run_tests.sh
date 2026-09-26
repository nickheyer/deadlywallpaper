#!/usr/bin/env bash
# End-to-end tests for the com.lively.wallpaper Plasma plugin, without Plasma:
#   (a) qmllint on every QML file,
#   (b) generated test media,
#   (c) for each kind: fake_core.py (scripted Lively core over WebSocket) + harness.qml
#       (LivelyContent in a 960x540 window), asserting msg_wploaded, msg_screenshot,
#       the screenshot pixels, the JS bridge, host_mpv_command and suspend/resume behaviour,
#   (d) span mode: three windows (full frame, left half, right half of one virtual screen)
#       frozen on the same video frame, their screenshots compared by span_compare.py.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PKG="$HERE/../com.lively.wallpaper"
QML="${QML:-/usr/lib/qt6/bin/qml}"
QMLLINT="${QMLLINT:-/usr/lib/qt6/bin/qmllint}"
FFMPEG="${FFMPEG:-ffmpeg}"
WORK="${LIVELY_TEST_TMP:-$(mktemp -d /tmp/lively-plasma-test.XXXXXX)}"
KINDS="${KINDS:-video gif picture web span}"
mkdir -p "$WORK"

# Arch's Qt logs to journald when stderr is not a TTY; force stderr so the logs are captured.
export QT_FORCE_STDERR_LOGGING=1

echo "== work dir: $WORK"

echo "== (a) qmllint"
"$QMLLINT" "$PKG"/contents/ui/*.qml "$HERE/harness.qml"
echo "qmllint: ok"

echo "== (b) test media"
"$FFMPEG" -hide_banner -loglevel error -y -f lavfi -i testsrc=size=1280x720:rate=30 -t 5 -pix_fmt yuv420p "$WORK/test.mp4"
"$FFMPEG" -hide_banner -loglevel error -y -f lavfi -i testsrc=size=1280x720:rate=1 -frames:v 1 "$WORK/test.png"
"$FFMPEG" -hide_banner -loglevel error -y -f lavfi -i testsrc=size=640x360:rate=10 -t 2 "$WORK/test.gif"
ls -la "$WORK"/test.*

free_port() {
    python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])'
}

wait_port() {
    local i
    for i in $(seq 1 50); do
        if python3 -c 'import socket, sys; s = socket.socket(); s.settimeout(0.2); sys.exit(0 if s.connect_ex(("127.0.0.1", int(sys.argv[1]))) == 0 else 1)' "$1"; then
            return 0
        fi
        sleep 0.1
    done
    return 1
}

failures=0

# start_pair NAME KIND SOURCE SCENARIO SHOT_PREFIX [extra harness args...]
# Starts a fake core and a harness; records their PIDs in CORE_PID[NAME] / HARNESS_PID[NAME].
declare -A CORE_PID HARNESS_PID
start_pair() {
    local name="$1" kind="$2" source="$3" scenario="$4" prefix="$5"
    shift 5
    local port instance
    port="$(free_port)"
    instance="lively-test-$name"
    echo "-- $name: kind=$kind source=$source scenario=$scenario ws://127.0.0.1:$port/wallpaper/$instance $*"

    python3 "$HERE/fake_core.py" --port "$port" --instance "$instance" --kind "$kind" --work-dir "$WORK" \
        --scenario "$scenario" --shot-prefix "$prefix" > "$WORK/$name.core.log" 2>&1 &
    CORE_PID[$name]=$!
    if ! wait_port "$port"; then
        echo "fake core for $name did not start listening"
        kill "${CORE_PID[$name]}" 2>/dev/null || true
        return 1
    fi

    "$QML" "$HERE/harness.qml" -- --kind "$kind" --source "$source" --core "ws://127.0.0.1:$port" \
        --instance "$instance" --scaler uniformFill --volume 0 --interactive false --timeout 180000 "$@" \
        > "$WORK/$name.harness.log" 2>&1 &
    HARNESS_PID[$name]=$!
    return 0
}

# finish_pair NAME: waits for the fake core, then for the harness (which quits on cmd_close).
# Prints the core log and the harness diagnostics; returns 1 on any failure.
finish_pair() {
    local name="$1" core_rc=0 harness_rc=0 i
    wait "${CORE_PID[$name]}" || core_rc=$?
    for i in $(seq 1 50); do
        kill -0 "${HARNESS_PID[$name]}" 2>/dev/null || break
        sleep 0.1
    done
    if kill -0 "${HARNESS_PID[$name]}" 2>/dev/null; then
        echo "harness $name still running after the fake core finished; killing it"
        kill "${HARNESS_PID[$name]}" 2>/dev/null || true
        wait "${HARNESS_PID[$name]}" 2>/dev/null || true
        harness_rc=3
    else
        wait "${HARNESS_PID[$name]}" || harness_rc=$?
    fi
    echo "-- fake core log ($WORK/$name.core.log):"
    cat "$WORK/$name.core.log"
    echo "-- harness diagnostics ($WORK/$name.harness.log, message echo lines omitted):"
    grep -v -e "harness plugin->core" -e "harness core->plugin" "$WORK/$name.harness.log" | head -60 || true
    echo "-- $name: fake core exit code: $core_rc, harness exit code: $harness_rc"
    [ "$core_rc" -eq 0 ] && [ "$harness_rc" -eq 0 ]
}

run_case() {
    local kind="$1" source="$2"
    echo
    echo "== (c) kind=$kind"
    if start_pair "$kind" "$kind" "$source" full "$kind" && finish_pair "$kind"; then
        echo "RESULT kind=$kind: PASS"
    else
        echo "RESULT kind=$kind: FAIL"
        failures=$((failures + 1))
    fi
}

run_span_case() {
    local ok=1
    echo
    echo "== (d) span mode: virtual screen 960x540, windows full (960x540), left (0,0,480,540), right (480,0,480,540)"
    start_pair span-full video "$WORK/test.mp4" span span-full --width 960 --height 540 || ok=0
    start_pair span-left video "$WORK/test.mp4" span span-left --width 480 --height 540 --span 0,0,480,540,960,540 || ok=0
    start_pair span-right video "$WORK/test.mp4" span span-right --width 480 --height 540 --span 480,0,480,540,960,540 || ok=0
    finish_pair span-full || ok=0
    finish_pair span-left || ok=0
    finish_pair span-right || ok=0
    if [ "$ok" -eq 1 ] && python3 "$HERE/span_compare.py" "$WORK/span-full-1.png" "$WORK/span-left-1.png" "$WORK/span-right-1.png"; then
        echo "RESULT span: PASS"
    else
        echo "RESULT span: FAIL"
        failures=$((failures + 1))
    fi
}

for kind in $KINDS; do
    case "$kind" in
        video) run_case video "$WORK/test.mp4" ;;
        gif) run_case gif "$WORK/test.gif" ;;
        picture) run_case picture "$WORK/test.png" ;;
        web) run_case web "$HERE/web/index.html" ;;
        span) run_span_case ;;
        *) echo "unknown kind $kind"; failures=$((failures + 1)) ;;
    esac
done

echo
if [ "$failures" -eq 0 ]; then
    echo "ALL TESTS PASSED (screenshots and logs in $WORK)"
else
    echo "$failures TEST CASE(S) FAILED (screenshots and logs in $WORK)"
    exit 1
fi

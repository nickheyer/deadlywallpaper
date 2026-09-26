#!/bin/bash
# Live test of lively-mpv-host against the running Wayland session (see README.md).
# Plays a generated test pattern on $LIVELY_TEST_OUTPUT (default DP-3) for ~6 s, takes a
# screenshot through the stdin protocol and checks the stdout messages and the PNG.
set -u
BUILD_DIR=${BUILD_DIR:-build}
OUTPUT=${LIVELY_TEST_OUTPUT:-DP-3}
HOST=$BUILD_DIR/lively-mpv-host
VIDEO=$BUILD_DIR/test.mp4
SHOT=$(cd "$BUILD_DIR" && pwd)/shot.png
STDOUT_LOG=$BUILD_DIR/test-stdout.log
STDERR_LOG=$BUILD_DIR/test-stderr.log
failures=0

fail() {
    echo "FAIL: $*"
    failures=$((failures + 1))
}

pass() {
    echo "PASS: $*"
}

if [ ! -x "$HOST" ]; then
    echo "FAIL: $HOST is not built"
    exit 1
fi

ffmpeg -loglevel error -y -f lavfi -i testsrc=size=1280x720:rate=30 -t 5 -pix_fmt yuv420p "$VIDEO" || {
    echo "FAIL: ffmpeg could not generate $VIDEO"
    exit 1
}
rm -f "$SHOT" "$STDOUT_LOG" "$STDERR_LOG"

(
    echo '{"Type":9,"Volume":0}'
    sleep 4
    echo '{"Type":6,"Format":1,"FilePath":"'"$SHOT"'","Delay":0}'
    sleep 2
    echo '{"Type":5}'
) | timeout 25 "$HOST" --output "$OUTPUT" --verbose "$VIDEO" >"$STDOUT_LOG" 2>"$STDERR_LOG"
status=$?

if [ $status -eq 0 ]; then
    pass "host exited with code 0"
else
    fail "host exited with code $status (stderr in $STDERR_LOG)"
fi

grep -q '"Type":0' "$STDOUT_LOG" && pass "msg_hwnd received" || fail "no msg_hwnd (Type 0) on stdout"
grep -q '"Type":2,"Success":true' "$STDOUT_LOG" && pass "msg_wploaded Success:true received" || fail "no successful msg_wploaded (Type 2) on stdout"
grep -q '"Type":3,"FileName":"shot.png","Success":true' "$STDOUT_LOG" && pass "msg_screenshot Success:true received" || fail "no successful msg_screenshot (Type 3) on stdout"

if [ -f "$SHOT" ]; then
    size=$(stat -c %s "$SHOT")
    magic=$(head -c 8 "$SHOT" | od -An -tx1 | tr -d ' \n')
    if [ "$magic" = "89504e470d0a1a0a" ]; then
        pass "shot.png has a PNG signature"
    else
        fail "shot.png is not a PNG (magic $magic)"
    fi
    if [ "$size" -gt 20000 ]; then
        pass "shot.png is non-trivial ($size bytes)"
    else
        fail "shot.png is too small to hold the test pattern ($size bytes)"
    fi
else
    fail "shot.png was not written"
fi

if [ $failures -eq 0 ]; then
    echo "all checks passed"
    exit 0
fi
echo "$failures check(s) failed"
exit 1

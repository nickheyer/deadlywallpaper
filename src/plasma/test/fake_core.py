#!/usr/bin/env python3
"""Fake Lively core for the Plasma plugin tests.

A minimal RFC 6455 WebSocket server written with the standard library only
(python-websockets is not installed on the development machine). It accepts
/wallpaper/<instance>, drives the plugin through a scripted scenario, prints every
frame exchanged with a timestamp and asserts the plugin's replies, including the
pixels of the screenshots it requested (PIL). Exit code 0 means every assertion
passed; any failure prints "FAIL: ..." and exits with 1.

--scenario full (default) runs the whole message set for the kind; --scenario span pauses
the wallpaper at its first frame, takes one screenshot and closes (span_compare.py then
compares the screenshots of several windows).
"""

import argparse
import base64
import hashlib
import json
import os
import queue
import socket
import struct
import sys
import threading
import time

from PIL import Image, ImageChops, ImageStat

GUID = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
START = time.monotonic()


def now():
    return time.monotonic() - START


def log(text):
    print(f"[{now():7.3f}] {text}", flush=True)


class Disconnected(Exception):
    pass


class Connection:
    def __init__(self, sock, path):
        self.sock = sock
        self.path = path
        self.lock = threading.Lock()

    def send_frame(self, opcode, payload):
        header = bytes([0x80 | opcode])
        length = len(payload)
        if length < 126:
            header += bytes([length])
        elif length < 65536:
            header += bytes([126]) + struct.pack(">H", length)
        else:
            header += bytes([127]) + struct.pack(">Q", length)
        with self.lock:
            self.sock.sendall(header + payload)

    def send_text(self, text):
        self.send_frame(0x1, text.encode("utf-8"))

    def send_close(self):
        try:
            self.send_frame(0x8, struct.pack(">H", 1000))
        except OSError:
            pass

    def recv_exact(self, count):
        data = b""
        while len(data) < count:
            chunk = self.sock.recv(count - len(data))
            if not chunk:
                raise Disconnected()
            data += chunk
        return data

    def read_frame(self):
        first, second = self.recv_exact(2)
        fin = bool(first & 0x80)
        opcode = first & 0x0F
        masked = bool(second & 0x80)
        length = second & 0x7F
        if length == 126:
            length = struct.unpack(">H", self.recv_exact(2))[0]
        elif length == 127:
            length = struct.unpack(">Q", self.recv_exact(8))[0]
        mask = self.recv_exact(4) if masked else None
        payload = self.recv_exact(length)
        if mask:
            payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        return fin, opcode, payload


class Server(threading.Thread):
    """Accepts WebSocket clients and pushes ("open"|"message"|"closed"|"badpath", ...) events."""

    def __init__(self, port, events):
        super().__init__(daemon=True)
        self.events = events
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.listener.bind(("127.0.0.1", port))
        self.listener.listen(4)

    def run(self):
        while True:
            sock, _ = self.listener.accept()
            threading.Thread(target=self.serve_client, args=(sock,), daemon=True).start()

    def serve_client(self, sock):
        try:
            request = b""
            while b"\r\n\r\n" not in request:
                chunk = sock.recv(4096)
                if not chunk:
                    return
                request += chunk
            head = request.split(b"\r\n\r\n", 1)[0].decode("latin-1")
            lines = head.split("\r\n")
            method, target, _ = lines[0].split(" ", 2)
            headers = {}
            for line in lines[1:]:
                if ":" in line:
                    name, value = line.split(":", 1)
                    headers[name.strip().lower()] = value.strip()
            key = headers.get("sec-websocket-key")
            if method != "GET" or headers.get("upgrade", "").lower() != "websocket" or key is None:
                sock.sendall(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")
                sock.close()
                return
            accept = base64.b64encode(hashlib.sha1(key.encode("latin-1") + GUID).digest()).decode("ascii")
            sock.sendall(("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n"
                          "Connection: Upgrade\r\nSec-WebSocket-Accept: " + accept + "\r\n\r\n").encode("ascii"))
        except (OSError, ValueError):
            sock.close()
            return

        connection = Connection(sock, target)
        self.events.put(("open", connection, None))
        fragments = b""
        try:
            while True:
                fin, opcode, payload = connection.read_frame()
                if opcode == 0x1 or opcode == 0x0:
                    fragments += payload
                    if fin:
                        self.events.put(("message", connection, fragments.decode("utf-8")))
                        fragments = b""
                elif opcode == 0x8:
                    connection.send_close()
                    break
                elif opcode == 0x9:
                    connection.send_frame(0xA, payload)
        except (Disconnected, OSError):
            pass
        finally:
            try:
                sock.close()
            except OSError:
                pass
            self.events.put(("closed", connection, None))


def image_stats(path):
    image = Image.open(path)
    fmt = image.format
    rgb = image.convert("RGB")
    stat = ImageStat.Stat(rgb)
    red, green, blue = rgb.split()
    colorfulness = (ImageStat.Stat(ImageChops.difference(red, green)).mean[0]
                    + ImageStat.Stat(ImageChops.difference(green, blue)).mean[0]
                    + ImageStat.Stat(ImageChops.difference(red, blue)).mean[0]) / 3
    return {
        "format": fmt,
        "size": rgb.size,
        "mean": sum(stat.mean) / 3,
        "stddev": max(stat.stddev),
        "colorfulness": colorfulness,
    }


def image_difference(path_a, path_b):
    a = Image.open(path_a).convert("RGB")
    b = Image.open(path_b).convert("RGB")
    if a.size != b.size:
        return 255.0
    return sum(ImageStat.Stat(ImageChops.difference(a, b)).mean) / 3


class Scenario:
    def __init__(self, args):
        self.args = args
        self.events = queue.Queue()
        self.server = Server(args.port, self.events)
        self.connection = None
        self.opens = 0
        self.received = []  # (time, message dict)
        self.shots = 0

    # ---- plumbing ----------------------------------------------------------

    def fail(self, reason):
        log("FAIL: " + reason)
        if self.connection is not None:
            self.connection.send_close()
        sys.exit(1)

    def send(self, message):
        text = json.dumps(message)
        log("core -> plugin " + text)
        if self.connection is None:
            self.fail("cannot send, plugin is not connected")
        try:
            self.connection.send_text(text)
        except OSError as error:
            self.fail("send failed: " + str(error))

    def pump(self, timeout):
        """Handles one event; returns it (kind, payload) or None on timeout."""
        try:
            kind, connection, payload = self.events.get(timeout=timeout)
        except queue.Empty:
            return None
        if kind == "open":
            self.opens += 1
            log(f"plugin connected to {connection.path} (connection #{self.opens})")
            if connection.path != f"/wallpaper/{self.args.instance}":
                self.fail(f"unexpected WebSocket path {connection.path}")
            self.connection = connection
            return ("open", connection.path)
        if kind == "closed":
            log("plugin disconnected")
            if self.connection is connection:
                self.connection = None
            return ("closed", None)
        log("plugin -> core " + payload)
        try:
            message = json.loads(payload)
        except ValueError:
            self.fail("plugin sent a frame that is not JSON: " + payload)
        if not isinstance(message, dict) or not isinstance(message.get("Type"), int):
            self.fail("plugin message has no integer Type: " + payload)
        self.received.append((now(), message))
        return ("message", message)

    def wait_for(self, predicate, timeout, what):
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                self.fail(f"timed out after {timeout:.0f}s waiting for {what}")
            event = self.pump(remaining)
            if event is not None and predicate(event):
                return event

    def wait_message(self, msg_type, timeout, what, **fields):
        def match(event):
            if event[0] != "message" or event[1]["Type"] != msg_type:
                return False
            return all(event[1].get(k) == v for k, v in fields.items())
        return self.wait_for(match, timeout, what)[1]

    def wait_console(self, needle, timeout, since=0.0):
        """Waits until a msg_console received after `since` contains `needle`."""
        for t, message in self.received:
            if t >= since and message["Type"] == 1 and needle in str(message.get("Message")):
                return message
        return self.wait_message_where(
            lambda m: m["Type"] == 1 and needle in str(m.get("Message")), timeout, f"console message containing {needle!r}")

    def wait_message_where(self, predicate, timeout, what):
        return self.wait_for(lambda e: e[0] == "message" and predicate(e[1]), timeout, what)[1]

    def sleep(self, seconds):
        """Waits while still draining incoming messages."""
        deadline = time.monotonic() + seconds
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return
            self.pump(remaining)

    def console_lines(self, since, category=None):
        return [str(m.get("Message")) for t, m in self.received
                if t >= since and m["Type"] == 1 and (category is None or m.get("Category") == category)]

    def expect(self, condition, description):
        if condition:
            log("ok: " + description)
        else:
            self.fail(description)

    # ---- screenshots -------------------------------------------------------

    def screenshot(self, fmt, suffix, expect_success=True):
        self.shots += 1
        name = f"{self.args.shot_prefix}-{self.shots}.{suffix}"
        path = os.path.join(self.args.work_dir, name)
        if os.path.exists(path):
            os.remove(path)
        self.send({"Type": 6, "Format": fmt, "FilePath": path, "Delay": 0})
        reply = self.wait_message(3, 20, f"msg_screenshot for {name}", FileName=name)
        self.expect(reply.get("Success") is expect_success,
                    f"msg_screenshot {name} Success={reply.get('Success')} (expected {expect_success})")
        if expect_success:
            self.expect(os.path.isfile(path) and os.path.getsize(path) > 0, f"{name} exists on disk")
        return path

    def nontrivial(self, path, min_colorfulness=None):
        stats = image_stats(path)
        log(f"{os.path.basename(path)}: format={stats['format']} size={stats['size']} mean={stats['mean']:.1f} "
            f"stddev={stats['stddev']:.1f} colorfulness={stats['colorfulness']:.1f}")
        self.expect(stats["size"][0] >= 960 and stats["size"][1] >= 540, f"{os.path.basename(path)} has the window size")
        self.expect(8 < stats["mean"] < 247, f"{os.path.basename(path)} mean pixel value {stats['mean']:.1f} is not blank")
        self.expect(stats["stddev"] >= 10, f"{os.path.basename(path)} has structure (stddev {stats['stddev']:.1f})")
        if min_colorfulness is not None:
            self.expect(stats["colorfulness"] >= min_colorfulness,
                        f"{os.path.basename(path)} is colourful ({stats['colorfulness']:.1f} >= {min_colorfulness})")
        return stats

    # ---- scenario ----------------------------------------------------------

    def run(self):
        self.server.start()
        log(f"listening on ws://127.0.0.1:{self.args.port}/wallpaper/{self.args.instance} for kind {self.args.kind}")
        self.wait_for(lambda e: e[0] == "open", 30, "the plugin to connect")
        self.wait_message(2, 60, "msg_wploaded Success=true", Success=True)

        if self.args.scenario == "span":
            self.span_scenario()
        else:
            self.full_scenario()

        # Close the session: the plugin must disconnect and must not reconnect.
        self.send({"Type": 5})
        self.wait_for(lambda e: e[0] == "closed", 10, "the plugin to disconnect after cmd_close")
        opens_before = self.opens
        self.sleep(2.5)
        self.expect(self.opens == opens_before, "no reconnect after cmd_close")

        errors = [m for t, m in self.received if m["Type"] == 1 and m.get("Category") == 1]
        unexpected = [m["Message"] for m in errors if not str(m["Message"]).startswith("Screenshot rejected")]
        self.expect(not unexpected, "no unexpected error console messages: " + json.dumps(unexpected))
        log(f"PASS kind={self.args.kind} scenario={self.args.scenario}: {len(self.received)} messages received from the plugin")

    def span_scenario(self):
        # Freeze every window on the same frame so their screenshots can be compared.
        self.send({"Type": 100, "Command": ["set_property", "pause", True]})
        self.send({"Type": 100, "Command": ["seek", 0, "absolute-percent"]})
        self.sleep(1.2)
        shot = self.screenshot(1, "png")
        self.nontrivial(shot)

    def full_scenario(self):
        self.send({"Type": 9, "Volume": 50})
        self.send({"Type": 12, "Name": "saturation", "Value": 40.0, "Step": 1.0})

        if self.args.kind in ("video", "gif", "picture"):
            self.media_scenario()
        else:
            self.web_scenario()

        self.format_scenario()

    def media_scenario(self):
        kind = self.args.kind
        self.send({"Type": 12, "Name": "saturation", "Value": 0.0, "Step": 1.0})
        self.sleep(0.8)
        base = self.screenshot(1, "png")
        self.nontrivial(base, min_colorfulness=15)

        # saturation -100: MultiEffect must render a grey image.
        self.send({"Type": 12, "Name": "saturation", "Value": -100.0, "Step": 1.0})
        self.sleep(0.8)
        grey = self.screenshot(1, "png")
        grey_stats = self.nontrivial(grey)
        self.expect(grey_stats["colorfulness"] < 4, f"saturation -100 gives a grey image ({grey_stats['colorfulness']:.2f} < 4)")

        # lp_button IsDefault restores the defaults (the real core then re-sends the file values).
        self.send({"Type": 16, "Name": "reset", "IsDefault": True})
        self.sleep(0.8)
        restored = self.screenshot(1, "png")
        self.nontrivial(restored, min_colorfulness=15)

        # gamma +100 through the ShaderEffect brightens the picture.
        self.send({"Type": 12, "Name": "gamma", "Value": 100.0, "Step": 1.0})
        self.sleep(0.8)
        bright = self.screenshot(1, "png")
        bright_stats = self.nontrivial(bright)
        base_stats = image_stats(restored)
        self.expect(bright_stats["mean"] > base_stats["mean"] + 8,
                    f"gamma +100 brightens ({bright_stats['mean']:.1f} > {base_stats['mean']:.1f} + 8)")
        self.send({"Type": 16, "Name": "reset", "IsDefault": True})

        # hue +100 rotates the chroma by 180 degrees: still colourful, but different pixels.
        self.send({"Type": 12, "Name": "hue", "Value": 100.0, "Step": 1.0})
        self.sleep(0.8)
        hued = self.screenshot(1, "png")
        self.nontrivial(hued, min_colorfulness=15)
        hue_diff = image_difference(restored, hued)
        self.expect(hue_diff > 10, f"hue +100 changes the picture (mean abs diff {hue_diff:.1f} > 10)")
        self.send({"Type": 16, "Name": "reset", "IsDefault": True})

        # brightness -100 through MultiEffect blacks the picture out.
        self.send({"Type": 12, "Name": "brightness", "Value": -100.0, "Step": 1.0})
        self.sleep(0.8)
        dark = self.screenshot(1, "png")
        dark_stats = image_stats(dark)
        log(f"{os.path.basename(dark)}: mean={dark_stats['mean']:.1f}")
        self.expect(dark_stats["mean"] < 12, f"brightness -100 darkens ({dark_stats['mean']:.1f} < 12)")
        self.send({"Type": 16, "Name": "reset", "IsDefault": True})

        # contrast, speed and mute must be accepted without errors.
        self.send({"Type": 12, "Name": "contrast", "Value": 30.0, "Step": 1.0})
        self.send({"Type": 12, "Name": "speed", "Value": 2.0, "Step": 0.01})
        self.send({"Type": 18, "Name": "mute", "Value": True})
        self.sleep(0.5)
        self.send({"Type": 16, "Name": "reset", "IsDefault": True})

        # scaler none shows the 1280x720 source unscaled in a 960x540 window: a different crop.
        self.sleep(0.5)
        fitted = self.screenshot(1, "png")
        self.send({"Type": 19, "Name": "scaler", "Value": 0})
        self.sleep(0.8)
        unscaled = self.screenshot(1, "png")
        self.nontrivial(unscaled)
        if kind != "gif":
            scaler_diff = image_difference(fitted, unscaled)
            self.expect(scaler_diff > 3, f"scaler none changes the framing (mean abs diff {scaler_diff:.1f} > 3)")
        self.send({"Type": 16, "Name": "reset", "IsDefault": True})
        self.sleep(0.5)

        if kind in ("video", "gif"):
            self.mpv_command_scenario()

            # cmd_suspend pauses playback: two screenshots one second apart are identical.
            self.send({"Type": 7})
            self.sleep(0.8)
            paused_a = self.screenshot(1, "png")
            self.sleep(1.0)
            paused_b = self.screenshot(1, "png")
            paused_diff = image_difference(paused_a, paused_b)
            self.expect(paused_diff < 0.2, f"suspended playback is frozen (mean abs diff {paused_diff:.3f} < 0.2)")
            self.send({"Type": 8})
            self.sleep(0.8)
            playing_a = self.screenshot(1, "png")
            self.sleep(1.0)
            playing_b = self.screenshot(1, "png")
            playing_diff = image_difference(playing_a, playing_b)
            self.expect(playing_diff > 0.5, f"resumed playback advances (mean abs diff {playing_diff:.3f} > 0.5)")
        else:
            self.send({"Type": 7})
            self.sleep(0.5)
            self.send({"Type": 8})
            self.sleep(0.5)
            # host_mpv_command on a still image is answered with a log line, not an error.
            t_cmd = now()
            self.send({"Type": 100, "Command": ["seek", 50, "absolute-percent"]})
            line = self.wait_console("has no effect on a still image", 10, since=t_cmd)
            self.expect(line.get("Category") == 0, "still-image mpv command logged at Category 0")

        # cmd_reload re-opens the media and reports msg_wploaded again.
        before = now()
        self.send({"Type": 4})
        self.wait_message(2, 30, "msg_wploaded after cmd_reload", Success=True)
        self.sleep(1.0)
        reloaded = [m for t, m in self.received if t > before and m["Type"] == 2]
        self.expect(len(reloaded) == 1, f"exactly one msg_wploaded after cmd_reload ({len(reloaded)})")

    def mpv_command_scenario(self):
        """host_mpv_command (Type 100): seek and set_property through the video renderer."""
        self.send({"Type": 100, "Command": ["set_property", "pause", True]})
        self.send({"Type": 100, "Command": ["seek", 0, "absolute-percent"]})
        self.sleep(1.0)
        start_frame = self.screenshot(1, "png")
        self.nontrivial(start_frame)
        self.send({"Type": 100, "Command": ["seek", 50, "absolute-percent"]})
        self.sleep(1.0)
        middle_frame = self.screenshot(1, "png")
        self.nontrivial(middle_frame)
        seek_diff = image_difference(start_frame, middle_frame)
        self.expect(seek_diff > 3, f"seek to 50% shows a different frame than 0% (mean abs diff {seek_diff:.2f} > 3)")
        self.send({"Type": 100, "Command": ["seek", -50, "relative-percent"]})
        self.sleep(1.0)
        back_frame = self.screenshot(1, "png")
        back_diff = image_difference(start_frame, back_frame)
        self.expect(back_diff < 1, f"relative seek -50% returns to the first frame (mean abs diff {back_diff:.3f} < 1)")

        # Paused by set_property: the picture must not advance.
        self.sleep(1.0)
        still_frame = self.screenshot(1, "png")
        still_diff = image_difference(back_frame, still_frame)
        self.expect(still_diff < 0.2, f"set_property pause true holds the frame (mean abs diff {still_diff:.3f} < 0.2)")

        # Remaining set_property names are accepted without errors (audio cannot be observed here).
        self.send({"Type": 100, "Command": ["set_property", "volume", 30]})
        self.send({"Type": 100, "Command": ["set_property", "mute", True]})
        self.send({"Type": 100, "Command": ["set_property", "mute", False]})
        self.send({"Type": 100, "Command": ["set_property", "speed", 1.5]})
        self.send({"Type": 100, "Command": ["set_property", "aid", "no"]})
        self.send({"Type": 100, "Command": ["set_property", "aid", "1"]})

        # Unknown commands are logged (Category 0), never errors.
        t_unknown = now()
        self.send({"Type": 100, "Command": ["loadfile", "/nowhere.mp4"]})
        line = self.wait_console("Unsupported mpv command", 10, since=t_unknown)
        self.expect(line.get("Category") == 0, "unknown mpv command logged at Category 0")

        # Unpause and confirm playback advances again.
        self.send({"Type": 100, "Command": ["set_property", "pause", False]})
        self.sleep(0.8)
        moving_a = self.screenshot(1, "png")
        self.sleep(1.0)
        moving_b = self.screenshot(1, "png")
        moving_diff = image_difference(moving_a, moving_b)
        self.expect(moving_diff > 0.5, f"set_property pause false resumes playback (mean abs diff {moving_diff:.3f} > 0.5)")
        self.send({"Type": 16, "Name": "reset", "IsDefault": True})
        self.sleep(0.5)

    def web_scenario(self):
        self.wait_console("lively test page loaded", 20)
        started = now()
        self.wait_console('livelyPropertyListener ["saturation",40]', 10, since=started - 5)

        self.send({"Type": 13, "Name": "text", "Value": "hello world"})
        self.wait_console('livelyPropertyListener ["text","hello world"]', 10, since=started)
        self.send({"Type": 14, "Name": "drop", "Value": 2})
        self.wait_console('livelyPropertyListener ["drop",2]', 10, since=started)
        self.send({"Type": 17, "Name": "color", "Value": "#ff0000"})
        self.wait_console('livelyPropertyListener ["color","#ff0000"]', 10, since=started)
        self.send({"Type": 18, "Name": "check", "Value": True})
        self.wait_console('livelyPropertyListener ["check",true]', 10, since=started)
        self.send({"Type": 16, "Name": "btn", "IsDefault": False})
        self.wait_console('livelyPropertyListener ["btn",true]', 10, since=started)
        self.send({"Type": 15, "Name": "folder", "Value": "assets/present.txt"})
        self.wait_console('livelyPropertyListener ["folder","assets/present.txt"]', 10, since=started)
        t_missing = now()
        self.send({"Type": 15, "Name": "folder", "Value": "assets/missing.txt"})
        self.wait_console('livelyPropertyListener ["folder",null]', 10, since=t_missing)
        t_null = now()
        self.send({"Type": 15, "Name": "folder", "Value": None})
        self.wait_console('livelyPropertyListener ["folder",null]', 10, since=t_null)
        self.send({"Type": 19, "Name": "scaler", "Value": 2})

        self.send({"Type": 10, "Info": {"Cpu": 12.5, "Gpu": 3}})
        self.wait_console('sysinfo string {"Cpu":12.5,"Gpu":3}', 10, since=started)
        self.send({"Type": 11, "Info": {"Title": "Song", "Artist": "Band"}})
        self.wait_console('nowplaying string {"Title":"Song","Artist":"Band"}', 10, since=started)
        self.send({"Type": 20, "Data": [0.5] * 128})
        self.wait_console("audio array 128 0.5", 10, since=started)

        page = self.screenshot(1, "png")
        self.nontrivial(page, min_colorfulness=10)

        # Suspend: media paused, playback event fired, page snapshotted, hidden and frozen:
        # the page's 250 ms setInterval must stop ticking.
        t_suspend = now()
        self.send({"Type": 7})
        self.wait_console('playback string {"IsPaused":true}', 10, since=t_suspend)
        self.sleep(1.0)
        window_start = now()
        self.sleep(1.5)
        window_end = now()
        frozen_ticks = [m for t, m in self.received
                        if window_start < t < window_end and m["Type"] == 1 and str(m.get("Message")).startswith("tick")]
        self.expect(len(frozen_ticks) == 0, f"no page ticks while suspended ({len(frozen_ticks)} in 1.5 s)")
        suspended_shot = self.screenshot(1, "png")
        self.nontrivial(suspended_shot, min_colorfulness=10)

        t_resume = now()
        self.send({"Type": 8})
        self.wait_console('playback string {"IsPaused":false}', 10, since=t_resume)
        self.wait_console('nowplaying string {"Title":"Song","Artist":"Band"}', 10, since=t_resume)
        self.sleep(1.5)
        resumed_ticks = [m for t, m in self.received
                         if t > t_resume and m["Type"] == 1 and str(m.get("Message")).startswith("tick")]
        self.expect(len(resumed_ticks) >= 2, f"page ticks again after resume ({len(resumed_ticks)} ticks)")
        resumed_shot = self.screenshot(1, "png")
        self.nontrivial(resumed_shot, min_colorfulness=10)

        # cmd_reload reloads the page: msg_wploaded and the load log arrive again.
        t_reload = now()
        self.send({"Type": 4})
        self.wait_message(2, 30, "msg_wploaded after cmd_reload", Success=True)
        self.wait_console("lively test page loaded", 20, since=t_reload)
        self.sleep(1.0)
        reloaded = [m for t, m in self.received if t > t_reload and m["Type"] == 2]
        self.expect(len(reloaded) == 1, f"exactly one msg_wploaded after cmd_reload ({len(reloaded)})")

    def format_scenario(self):
        jpeg = self.screenshot(0, "jpg")
        stats = image_stats(jpeg)
        self.expect(stats["format"] == "JPEG", f"Format 0 wrote a JPEG file ({stats['format']})")
        # Format/suffix mismatch is an explicit failure, not a silently different file.
        t_bad = now()
        self.screenshot(1, "jpg", expect_success=False)
        self.wait_console("Screenshot rejected", 10, since=t_bad)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--instance", required=True)
    parser.add_argument("--kind", required=True, choices=["video", "gif", "picture", "web", "url", "videostream"])
    parser.add_argument("--work-dir", required=True, help="directory for the requested screenshots")
    parser.add_argument("--scenario", default="full", choices=["full", "span"])
    parser.add_argument("--shot-prefix", default=None, help="screenshot file name prefix (default: the kind)")
    args = parser.parse_args()
    if args.shot_prefix is None:
        args.shot_prefix = args.kind
    os.makedirs(args.work_dir, exist_ok=True)
    Scenario(args).run()


if __name__ == "__main__":
    main()

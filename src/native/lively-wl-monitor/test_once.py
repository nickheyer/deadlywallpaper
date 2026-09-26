#!/usr/bin/env python3
"""Checks one `lively-wl-monitor --once` run, fed on stdin.

The checks are structural so they pass on any Wayland session: every reported
output must have a unique name, a positive logical size and a positive scale,
and the toplevel list must contain exactly one activated window.
"""
import json
import sys


OUTPUT_FIELDS = {
    "name": str, "description": str, "make": str, "model": str,
    "x": int, "y": int, "width": int, "height": int, "scale": (int, float),
    "transform": int, "physical_width_mm": int, "physical_height_mm": int,
    "refresh_mhz": int,
}
TOPLEVEL_FIELDS = {
    "id": str, "app_id": str, "title": str, "activated": bool, "fullscreen": bool,
    "maximized": bool, "minimized": bool, "skip_taskbar": bool, "outputs": list,
    "virtual_desktops": list, "activities": list, "on_current_desktop": bool,
}


def fail(message):
    print("FAIL: " + message, file=sys.stderr)
    sys.exit(1)


def check_fields(obj, fields, what):
    for key, kind in fields.items():
        if key not in obj:
            fail("%s is missing %r: %s" % (what, key, obj))
        if not isinstance(obj[key], kind) or (kind is int and isinstance(obj[key], bool)):
            fail("%s field %r has the wrong type: %r" % (what, key, obj[key]))


lines = [line for line in sys.stdin.read().split("\n") if line]
events = [json.loads(line) for line in lines]
if not events:
    fail("no output at all")
if events[0].get("event") != "capabilities":
    fail("first line is not the capabilities event: %s" % lines[0])
capabilities = events[0]
for key in ("layer_shell", "plasma_shell"):
    if not isinstance(capabilities.get(key), bool):
        fail("capabilities.%s is not a boolean" % key)
if capabilities.get("toplevel_protocol") not in ("wlr", "plasma", "none"):
    fail("capabilities.toplevel_protocol is invalid: %r" % capabilities.get("toplevel_protocol"))

outputs_events = [e for e in events if e.get("event") == "outputs"]
if len(outputs_events) != 1:
    fail("expected exactly one outputs event, got %d" % len(outputs_events))
outputs = {}
for output in outputs_events[0]["outputs"]:
    check_fields(output, OUTPUT_FIELDS, "output")
    outputs[output["name"]] = output
if not outputs:
    fail("no outputs reported")
if len(outputs) != len(outputs_events[0]["outputs"]):
    fail("output names are not unique: %s" % [o["name"] for o in outputs_events[0]["outputs"]])
for name, output in outputs.items():
    if not name:
        fail("an output has an empty name: %s" % output)
    if output["width"] <= 0 or output["height"] <= 0:
        fail("%s has a non-positive logical size: %dx%d" % (name, output["width"], output["height"]))
    if output["scale"] <= 0:
        fail("%s has a non-positive scale: %r" % (name, output["scale"]))

toplevel_events = [e for e in events if e.get("event") == "toplevels"]
if capabilities["toplevel_protocol"] == "none":
    fail("no toplevel protocol was bound; on KWin run `make register-kde` first")
if len(toplevel_events) != 1:
    fail("expected exactly one toplevels event, got %d" % len(toplevel_events))
if not isinstance(toplevel_events[0].get("show_desktop"), bool):
    fail("toplevels.show_desktop is not a boolean: %r" % toplevel_events[0].get("show_desktop"))
toplevels = toplevel_events[0]["toplevels"]
for toplevel in toplevels:
    check_fields(toplevel, TOPLEVEL_FIELDS, "toplevel")
    if not all(isinstance(name, str) for name in toplevel["outputs"]):
        fail("toplevel outputs must be strings: %r" % toplevel["outputs"])
    for key in ("virtual_desktops", "activities"):
        if not all(isinstance(name, str) for name in toplevel[key]):
            fail("toplevel %s must be strings: %r" % (key, toplevel[key]))
    if capabilities["toplevel_protocol"] == "wlr" and (toplevel["virtual_desktops"] or toplevel["activities"] or not toplevel["on_current_desktop"]):
        fail("wlr toplevels carry no desktop or activity membership: %s" % toplevel)
    geometry = toplevel.get("geometry", "missing")
    if geometry == "missing":
        fail("toplevel is missing geometry: %s" % toplevel)
    if geometry is not None and not (
        isinstance(geometry, list) and len(geometry) == 4
        and all(isinstance(v, int) and not isinstance(v, bool) for v in geometry)
    ):
        fail("toplevel geometry must be null or four integers: %r" % geometry)
active = [t for t in toplevels if t["activated"]]
if not active:
    fail("no toplevel has activated=true among %d toplevels" % len(toplevels))
if capabilities["toplevel_protocol"] == "plasma" and any(not t["on_current_desktop"] for t in active):
    fail("the activated window must be on the current virtual desktop: %s" % active)

if len(events) != 3:
    fail("expected exactly 3 lines from --once, got %d" % len(events))

print("OK: protocol=%s outputs=%s toplevels=%d active=%s" % (
    capabilities["toplevel_protocol"], sorted(outputs), len(toplevels),
    ", ".join("%s (%s)" % (t["app_id"], t["title"]) for t in active)))

#!/usr/bin/env python3
"""Checks the span-mode screenshots of the Plasma plugin tests.

    span_compare.py FULL.png LEFT.png RIGHT.png

FULL is a non-span window showing the whole wallpaper, LEFT and RIGHT are span windows
showing the left and right halves of the same virtual screen at the same frame. Each half
must match its half of FULL and must not match the other half. Exit code 0 on success,
1 with a "FAIL: ..." line otherwise.
"""

import sys

from PIL import Image, ImageChops, ImageStat


MATCH_MAX = 12.0     # mean absolute difference allowed for the matching half (resampling noise)
MISMATCH_MIN = 30.0  # mean absolute difference required against the other half


def mean_abs_diff(a, b):
    if a.size != b.size:
        b = b.resize(a.size, Image.LANCZOS)
    return sum(ImageStat.Stat(ImageChops.difference(a, b)).mean) / 3


def main():
    if len(sys.argv) != 4:
        print(__doc__)
        return 2
    full = Image.open(sys.argv[1]).convert("RGB")
    left = Image.open(sys.argv[2]).convert("RGB")
    right = Image.open(sys.argv[3]).convert("RGB")
    half = full.width // 2
    full_left = full.crop((0, 0, half, full.height))
    full_right = full.crop((full.width - half, 0, full.width, full.height))
    print(f"full {full.size} left {left.size} right {right.size}")

    checks = [
        ("left slice vs full left half", mean_abs_diff(left, full_left), "<=", MATCH_MAX),
        ("right slice vs full right half", mean_abs_diff(right, full_right), "<=", MATCH_MAX),
        ("left slice vs full right half", mean_abs_diff(left, full_right), ">=", MISMATCH_MIN),
        ("right slice vs full left half", mean_abs_diff(right, full_left), ">=", MISMATCH_MIN),
    ]
    failed = False
    for label, value, op, limit in checks:
        ok = value <= limit if op == "<=" else value >= limit
        print(f"{'ok' if ok else 'FAIL'}: {label}: mean abs diff {value:.2f} {op} {limit}")
        failed = failed or not ok
    if failed:
        print("FAIL: span slices do not match the full frame")
        return 1
    print("PASS: span slices match the full frame")
    return 0


if __name__ == "__main__":
    sys.exit(main())

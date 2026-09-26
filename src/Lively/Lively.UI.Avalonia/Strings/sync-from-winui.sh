#!/usr/bin/env bash
# Copies the WinUI .resw string tables into this folder as .resx files.
# .resw and .resx share the same XML schema, so the copy is verbatim:
#   Strings/en-US/Resources.resw  -> Resources.resx        (neutral / fallback culture)
#   Strings/<culture>/Resources.resw -> Resources.<culture>.resx
# The generated files are committed; the build never runs this script.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
src="$here/../../Lively.UI.WinUI/Strings"

if [ ! -d "$src" ]; then
  echo "WinUI strings folder not found: $src" >&2
  exit 1
fi

rm -f "$here"/Resources.resx "$here"/Resources.*.resx
count=0
for dir in "$src"/*/; do
  culture="$(basename "$dir")"
  resw="$dir/Resources.resw"
  [ -f "$resw" ] || continue
  if [ "$culture" = "en-US" ]; then
    out="$here/Resources.resx"
  else
    out="$here/Resources.$culture.resx"
  fi
  cp "$resw" "$out"
  count=$((count + 1))
done
echo "Synchronised $count string tables into $here"

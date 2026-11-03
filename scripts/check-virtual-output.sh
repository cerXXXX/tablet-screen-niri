#!/bin/bash
# After logging into the patched niri: prove the virtual output works and
# measure DMA-BUF capture through ext-image-copy-capture.
#
#   ./scripts/check-virtual-output.sh            # 2400x1500@60
#   ./scripts/check-virtual-output.sh 3000 1872 60
#
# Creates the output, puts it to the right of the other screens, prints what
# niri reports, then runs moreland's capture probe against it. The output is
# left in place so the window-moving test can be done by hand; remove it with
# `niri msg remove-virtual-output tablet`.
set -uo pipefail

W=${1:-2400}
H=${2:-1500}
R=${3:-60}
NAME=tablet
PROBE=$HOME/tablet-screen/moreland/target/release/capture-probe

echo "=== niri"
niri msg version || { echo "niri is not reachable — is this a niri session?"; exit 1; }

echo
echo "=== create-virtual-output ${W}x${H}@${R}"
niri msg create-virtual-output --name "$NAME" --width "$W" --height "$H" --refresh-rate "$R" || true
sleep 1
niri msg output "$NAME" on || true
sleep 1

echo
echo "=== place it to the right of the other screens"
POS=$(niri msg -j outputs | python3 -c '
import json, sys
d = json.load(sys.stdin)
right = 0.0
top = 0.0
for name, o in d.items():
    if name == "'"$NAME"'":
        continue
    l = o.get("logical")
    if not l:
        continue
    right = max(right, l["x"] + l["width"])
    top = min(top, l["y"])
print(int(right), int(top))
')
echo "position set $POS"
niri msg output "$NAME" position set $POS || true
sleep 1

echo
echo "=== niri view"
niri msg outputs | sed -n "/$NAME/,/^\$/p"

echo
echo "=== casts (screencast clients niri knows about)"
niri msg casts || true

echo
echo "=== capture probe: ext-image-copy-capture + DMA-BUF, 240 frames"
if [ -x "$PROBE" ]; then
    "$PROBE" "$NAME" --dmabuf --frames 240 2>&1 | tail -25
else
    echo "probe not built: $PROBE"
fi

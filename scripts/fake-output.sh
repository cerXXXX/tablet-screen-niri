#!/bin/bash
# Turn a spare DRM connector into a "monitor" that niri can see, and back.
#
# This writes to the drm-core debugfs knobs (force / edid_override) — no driver
# code is touched, no module is rebuilt, and `off` restores the previous state.
#
#   sudo ./scripts/fake-output.sh HDMI-A-1 edid/tablet-3000x1872.bin on
#   sudo ./scripts/fake-output.sh HDMI-A-1 - off
#
# `on` usually needs a nudge before the compositor re-reads the connector: the
# script pokes udev, then asks niri to enable the output. If niri still does not
# list it, log out and back in — the compositor probes every connector at
# startup, so no reboot and no kernel parameter is needed for this test.
set -euo pipefail

conn="${1:?connector name, e.g. HDMI-A-1 or DP-3}"
edid="${2:-}"
action="${3:-on}"

# Talk to the user's compositor as the user, not as root (sudo drops NIRI_SOCKET).
as_user() {
    [ -n "${SUDO_UID:-}" ] || { "$@"; return; }
    local sock
    sock=$(ls /run/user/"$SUDO_UID"/niri.*.sock 2>/dev/null | head -1)
    runuser -u "$(id -un "$SUDO_UID")" -- env \
        XDG_RUNTIME_DIR=/run/user/"$SUDO_UID" \
        WAYLAND_DISPLAY="$(basename "${sock:-wayland-1}" | sed 's/^niri\.//; s/\.sock$//; s/\.sock\././')" \
        NIRI_SOCKET="$sock" "$@"
}

dir="$(find /sys/kernel/debug/dri -maxdepth 3 -type d -name "$conn" 2>/dev/null | head -1)"
if [ -z "$dir" ]; then
    echo "no debugfs entry for $conn under /sys/kernel/debug/dri" >&2
    echo "hint: mount -t debugfs none /sys/kernel/debug" >&2
    exit 1
fi
echo "debugfs: $dir"

case "$action" in
on)
    [ -f "$edid" ] || { echo "edid file not found: $edid" >&2; exit 1; }
    echo "EDID $edid ($(stat -c%s "$edid") bytes) -> edid_override"
    cat "$edid" > "$dir/edid_override"
    echo on > "$dir/force"
    ;;
off)
    # drm_edid_override_reset() only accepts exactly 5 bytes ("reset"), and
    # `echo` appends a newline, which the kernel rejects with EINVAL.
    printf reset > "$dir/edid_override" || echo "edid reset failed"
    # 12-byte kernel buffer: "unspecified\n" is one byte too long.
    printf unspecified > "$dir/force"
    ;;
*)
    echo "action must be on or off" >&2; exit 1 ;;
esac

for c in /sys/class/drm/card[0-9]; do
    [ -e "$c" ] && udevadm trigger --action=change "$c"
done
udevadm settle || true
sleep 1

echo
echo "=== kernel view ==="
for p in /sys/class/drm/*-"$conn"; do
    printf '%s  status=%s enabled=%s edid_bytes=%s\n' \
        "$(basename "$p")" \
        "$(cat "$p/status" 2>/dev/null)" \
        "$(cat "$p/enabled" 2>/dev/null)" \
        "$(stat -c%s "$p/edid" 2>/dev/null || echo 0)"
    echo "modes:"
    sed 's/^/  /' "$p/modes" 2>/dev/null || true
done

if [ "$action" = on ]; then
    as_user niri msg output "$conn" on 2>&1 | sed 's/^/niri: /' || true
fi

echo
echo "=== niri view ==="
as_user niri msg outputs 2>&1 | sed -n "/\"$conn\"/,/^$/p" || echo "niri not reachable"

echo
echo "=== dmesg (drm) ==="
dmesg 2>/dev/null | grep -iE "i915|drm" | tail -8 || true

if [ "$action" = on ]; then
    cat <<'EOF'

If `modes:` above is empty, the kernel rejected the EDID's timing and there is
nothing to enable. If it is not empty but niri shows the output as Disabled,
log out and back in.
EOF
fi

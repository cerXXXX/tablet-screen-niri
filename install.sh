#!/usr/bin/env bash
# Installs everything this tablet-as-a-second-screen setup needs, from this
# repository, so that nothing is lost and `pacman -Syu` cannot take it away:
#
#   niri-lock-wallpaper-tablet   niri with BOTH patches: the wallpaper under a
#                                see-through lock surface and the virtual
#                                outputs (`niri msg create-virtual-output`)
#   uscreen-dmabuf               UScreen fork: capture the compositor's
#                                DMA-BUFs and encode them on the GPU
#   dms plugin "Tablet Screen"   a bar pill with an on/off switch and
#                                resolution presets
#   the niri input mapping       `input { touch/tablet { map-to-output } }`,
#                                without which touch lands on the focused
#                                screen instead of the tablet
#
# Safe to rerun: an installed package at the same version is not rebuilt, the
# plugin is copied over, the input mapping is only added when it is missing.
#
# usage: ./install.sh [--no-niri] [--rebuild-niri] [--no-uscreen]
#                     [--rebuild-uscreen] [--no-plugin] [--no-config]
#                     [--no-restart]
# Run it as your user; it asks for sudo itself where it needs it.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
plugins_dir="${XDG_CONFIG_HOME:-$HOME/.config}/DankMaterialShell/plugins"
niri_config="${XDG_CONFIG_HOME:-$HOME/.config}/niri/config.kdl"
uscreen_config="${XDG_CONFIG_HOME:-$HOME/.config}/uscreen/config.toml"

do_niri=1
rebuild_niri=0
do_uscreen=1
rebuild_uscreen=0
do_plugin=1
do_config=1
do_restart=1

for arg in "$@"; do
    case "$arg" in
        --no-niri) do_niri=0 ;;
        --rebuild-niri) rebuild_niri=1 ;;
        --no-uscreen) do_uscreen=0 ;;
        --rebuild-uscreen) rebuild_uscreen=1 ;;
        --no-plugin) do_plugin=0 ;;
        --no-config) do_config=0 ;;
        --no-restart) do_restart=0 ;;
        -h | --help)
            sed -n '2,25p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "unknown option: $arg" >&2
            exit 1
            ;;
    esac
done

say() { printf '\n==> %s\n' "$*"; }

pkg_version() {
    # version of the PKGBUILD in $1, without needing makepkg to parse the file
    (
        cd "$1"
        # shellcheck disable=SC1091
        . ./PKGBUILD
        printf '%s-%s' "$pkgver" "$pkgrel"
    )
}

installed_version() {
    pacman -Q --quiet "$1" 2>/dev/null && pacman -Q "$1" 2>/dev/null | awk '{print $2}' || true
}

install_pkg() {
    local dir=$1 name=$2 rebuild=$3
    shift 3
    local stale_names=("$@")
    local want have
    want=$(pkg_version "$dir")
    have=$(pacman -Q "$name" 2>/dev/null | awk '{print $2}' || true)
    if [ "$have" = "$want" ] && [ "$rebuild" -eq 0 ]; then
        say "$name $want already installed"
        return 0
    fi
    say "building and installing $name $want${have:+ (replacing $have)}"
    # `replaces` only fires when the new package is *newer* than what is
    # installed, and a bare conflict makes pacman ask a [y/N] question that
    # `--noconfirm` answers "no" to, aborting the transaction. Removing the
    # old name first makes the transition deterministic.
    for stale in "${stale_names[@]}"; do
        # `pacman -Q name` also answers for a package that merely *provides*
        # that name: it found "uscreen" behind uscreen-dmabuf and then asked
        # pacman to remove a target that does not exist, which aborted the
        # install. Only a real package name counts.
        if pacman -Qq | grep -qx "$stale"; then
            say "removing the package this replaces: $stale"
            sudo pacman -Rdd --noconfirm "$stale" || true
        fi
    done
    ( cd "$dir" && makepkg -si --noconfirm )
    niri_updated=1
}

niri_updated=0

if [ "$do_niri" -eq 1 ]; then
    install_pkg "$here/niri" niri-lock-wallpaper-tablet "$rebuild_niri" niri-tablet niri-maintest
fi

if [ "$do_uscreen" -eq 1 ]; then
    install_pkg "$here/uscreen" uscreen-dmabuf "$rebuild_uscreen" uscreen
    # A development symlink or drop-in pointed at a build directory; the package
    # now owns /usr/bin/uscreen, and a stale symlink in ~/.local/bin would
    # shadow it in every shell that has it first in PATH.
    if [ -L "$HOME/.local/bin/uscreen" ]; then
        case "$(readlink -f "$HOME/.local/bin/uscreen")" in
            "$here"/*)
                say "removing the development symlink ~/.local/bin/uscreen"
                rm -f "$HOME/.local/bin/uscreen"
                ;;
        esac
    fi
    # The development drop-in pointed at a build directory; the package now
    # owns /usr/bin/uscreen, so it must go or the unit runs the wrong binary.
    if [ -f "$HOME/.config/systemd/user/uscreen.service.d/override.conf" ]; then
        say "removing the development systemd drop-in"
        rm -f "$HOME/.config/systemd/user/uscreen.service.d/override.conf"
        rmdir --ignore-fail-on-non-empty "$HOME/.config/systemd/user/uscreen.service.d" 2>/dev/null || true
        systemctl --user daemon-reload
    fi
fi

if [ "$do_plugin" -eq 1 ]; then
    say "installing the DMS plugin 'Tablet Screen'"
    mkdir -p "$plugins_dir/tabletScreen"
    cp -f "$here"/dms-plugin/tabletScreen/* "$plugins_dir/tabletScreen/"
    echo "    add it in DMS: Settings → Plugins → Tablet Screen"
fi

if [ "$do_config" -eq 1 ]; then
    # The tablet output has to be named in the input mapping, or niri stretches
    # touch and pen across whichever screen is focused (a tap on the tablet
    # then lands on the laptop).
    if [ -f "$niri_config" ] && ! grep -q 'map-to-output "tablet"' "$niri_config"; then
        say "adding the touch/tablet mapping to $(basename "$niri_config")"
        cp -f "$niri_config" "$niri_config.bak-tablet-$(date +%Y%m%d%H%M%S)"
        python3 - "$niri_config" <<'PY'
import sys, pathlib
p = pathlib.Path(sys.argv[1])
s = p.read_text()
block = '''    // The tablet's second screen: its touchscreen and pen belong to that
    // output, not to whichever screen happens to be focused. The output only
    // exists while the tablet is streaming; while it is absent, niri falls
    // back to the focused output.
    touch {
        map-to-output "tablet"
    }
    tablet {
        map-to-output "tablet"
    }
'''
s = s.replace("input {\n", "input {\n" + block, 1)
p.write_text(s)
PY
        niri_updated=1
    fi

    # Keys this fork needs are ensured one by one, not only when the file is
    # missing: the stock uscreen rewrites config.toml whenever it saves
    # settings, and it knows nothing about these keys — running it once (for
    # example while both packages were installed) silently dropped them, and
    # the daemon then fell back to the EVDI path.
    if [ -x "$here/tools/uscreen-config.py" ] && [ -f "$uscreen_config" ]; then
        ensure_key() {
            if ! "$here/tools/uscreen-config.py" get "$1" >/dev/null 2>&1; then
                say "config: adding $1 = $2 to $(basename "$uscreen_config")"
                "$here/tools/uscreen-config.py" set "$1" "$2"
            fi
        }
        ensure_key capture wayland
        ensure_key output_name tablet
        ensure_key scale 1.5
        ensure_key cursor true
        ensure_key auto_resolution false
    fi

    if [ ! -f "$uscreen_config" ]; then
        say "writing a default $uscreen_config"
        mkdir -p "$(dirname "$uscreen_config")"
        cat > "$uscreen_config" <<'EOF'
# UScreen with the DMA-BUF backend: the compositor hands out its own GPU
# buffers and the iGPU encodes them. See this repository's README.
encoder = "h264_vaapi"
capture = "wayland"
output_name = "tablet"
fps = 60
width = 2400
height = 1500
quality = 18
bitrate = 20000
stream_scale = 1
scale = 1.5
cursor = true
auto_resolution = false
position = "right"
pen_only = false
EOF
    fi
fi

if [ "$do_restart" -eq 1 ]; then
    say "restarting the UScreen daemon"
    systemctl --user daemon-reload
    systemctl --user enable --now uscreen.service
    if systemctl --user is-active --quiet uscreen.service; then
        systemctl --user restart uscreen.service
    fi
fi

if [ "$niri_updated" -eq 1 ]; then
    cat <<'EOF'

niri was installed or its config changed: log out and back in for it to take
effect (`niri msg version` should say "lock-wallpaper+virtual-output").
EOF
fi

say "done"
echo "    doctor:  uscreen doctor"
echo "    on/off:  uscreen display toggle   (or the DMS pill)"

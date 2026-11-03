#!/usr/bin/env python3
"""Generate a minimal EDID 1.4 blob with a single detailed timing.

Usage: mk-edid.py WIDTH HEIGHT REFRESH OUT.bin [--mm WxH] [--name TEXT]

Timings come from `cvt -r` (reduced blanking) so they match what the kernel
would compute for a custom mode. The blob is what gets written into
/sys/kernel/debug/dri/<card>/<connector>/edid_override so that a *forced*
connector looks like a real monitor to niri.
"""
import subprocess
import sys

HEADER = bytes([0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00])


def modeline(w: int, h: int, r: int):
    """Return (pclk_hz, htotal, vtotal, hfront, hsync, vfront, vsync, +h, +v)."""
    out = subprocess.run(["cvt", str(w), str(h), str(r), "-r"],
                         capture_output=True, text=True, check=True).stdout
    line = [l for l in out.splitlines() if l.startswith("Modeline")][0]
    # Modeline "name" 365.00 3000 3048 3080 3160 1872 1875 1885 1926 +hsync -vsync
    parts = line.split()
    pclk = round(float(parts[2]) * 1_000_000)
    nums = [int(x) for x in parts[3:11]]
    hact, hsyn_start, hsyn_end, htotal, vact, vsyn_start, vsyn_end, vtotal = nums
    assert hact == w and vact == h, (nums, w, h)
    return (pclk, htotal, vtotal, hsyn_start - hact, hsyn_end - hsyn_start,
            vsyn_start - vact, vsyn_end - vsyn_start,
            "+hsync" in parts, "+vsync" in parts)


def dtd(pclk, htotal, vtotal, hfront, hsync, vfront, vsync, hp, vp, w, h, w_mm, h_mm):
    hact, vact = w, h
    hblank, vblank = htotal - hact, vtotal - vact
    b = bytearray(18)
    b[0] = (pclk // 10_000) & 0xFF
    b[1] = (pclk // 10_000) >> 8
    b[2] = hact & 0xFF
    b[3] = hblank & 0xFF
    b[4] = ((hact >> 8) << 4) | (hblank >> 8)
    b[5] = vact & 0xFF
    b[6] = vblank & 0xFF
    b[7] = ((vact >> 8) << 4) | (vblank >> 8)
    b[8] = hfront & 0xFF
    b[9] = hsync & 0xFF
    b[10] = ((vfront & 0x0F) << 4) | (vsync & 0x0F)
    b[11] = (((hfront >> 8) & 0x03) << 6) | (((hsync >> 8) & 0x03) << 4) \
            | (((vfront >> 4) & 0x03) << 2) | ((vsync >> 4) & 0x03)
    b[12] = w_mm & 0xFF
    b[13] = h_mm & 0xFF
    b[14] = (((w_mm >> 8) & 0x0F) << 4) | ((h_mm >> 8) & 0x0F)
    b[15] = 0        # horizontal border
    b[16] = 0        # vertical border
    flags = 0x18  # digital separate sync
    if hp:
        flags |= 0x02
    if vp:
        flags |= 0x04
    b[17] = flags
    return bytes(b)


def desc(kind: int, payload: bytes) -> bytes:
    """18-byte display descriptor: 4-byte header plus a payload."""
    assert len(payload) == 13, (kind, len(payload))
    return bytes([0, 0, 0, kind, 0]) + payload


def cta_with_hdmi_vsdb(max_tmds_mhz: int) -> bytes:
    """CTA-861 extension whose only data block is an HDMI VSDB.

    Without a VSDB the sink looks like plain DVI/HDMI 1.4 and i915 caps the
    pixel clock at 165 MHz. The VSDB's Max_TMDS_Clock byte is what lifts that
    cap (5 MHz units).
    """
    vsdb = bytes([0x03, 0x0C, 0x00,          # IEEE OUI 0x000C03, LSB first
                  0x10, 0x00,                # source physical address 1.0.0.0
                  0x00,                      # no deep colour / AI flags
                  max(1, min(255, max_tmds_mhz // 5))])
    block = bytes([(0x03 << 5) | len(vsdb)]) + vsdb
    block += bytes([(0x02 << 5) | 1, 0x01])   # Video Data Block: VIC 1
    dtd_offset = 4 + len(block)

    ext = bytearray(128)
    ext[0] = 0x02                 # CTA-861 extension
    ext[1] = 0x03                 # revision 3
    ext[2] = dtd_offset           # start of the (empty) DTD area
    ext[3] = 0x30                # YCbCr 4:4:4 and 4:2:2 supported
    ext[4:dtd_offset] = block
    ext[127] = (256 - (sum(ext[:127]) % 256)) % 256
    return bytes(ext)


def main():
    if len(sys.argv) < 5:
        print(__doc__)
        return 1
    w, h, r = (int(x) for x in sys.argv[1:4])
    out = sys.argv[4]
    mm = None
    name = "Tablet"
    hdmi_mhz = 600
    rest = sys.argv[5:]
    for i, a in enumerate(rest):
        if a == "--mm":
            mm = tuple(int(x) for x in rest[i + 1].lower().split("x"))
        if a == "--name":
            name = rest[i + 1]
        if a == "--no-hdmi-vsdb":
            hdmi_mhz = 0
        if a == "--hdmi-max-mhz":
            hdmi_mhz = int(rest[i + 1])

    pclk, htotal, vtotal, hfront, hsync, vfront, vsync, hp, vp = modeline(w, h, r)
    if mm is None:
        # assume a 16:10 tablet, ~8.8" diagonal -> mm from aspect and panel size
        diag_mm = 224.0
        k = diag_mm / ((w ** 2 + h ** 2) ** 0.5)
        mm = (round(w * k), round(h * k))

    blob = bytearray()
    blob += HEADER
    mfg = ((ord("N") - 64) << 10) | ((ord("R") - 64) << 5) | (ord("I") - 64)
    blob += bytes([mfg >> 8, mfg & 0xFF])          # manufacturer "NRI"
    blob += bytes([0x34, 0x12])                    # product code
    blob += bytes([0, 0, 0, 1])                    # serial
    blob += bytes([1, 36])                         # week 1, year 2026
    blob += bytes([1, 4])                          # EDID 1.4
    blob += bytes([0xA2])                          # digital, 8 bpc, HDMI-a
    blob += bytes([mm[0] // 10, mm[1] // 10])      # size in cm
    blob += bytes([120, 0x0A])                     # gamma 2.2, RGB, preferred timing
    blob += bytes([0xEE, 0x91, 0xA3, 0x54, 0x4C, 0x99, 0x26, 0x0F, 0x50, 0x54])
    blob += bytes([0x20, 0x00, 0x00])              # established: 640x480@60
    blob += bytes([0x01] * 16)                     # standard timings: unused
    assert len(blob) == 54, len(blob)

    blob += dtd(pclk, htotal, vtotal, hfront, hsync, vfront, vsync,
                hp, vp, w, h, mm[0], mm[1])
    blob += desc(0xFC, (name.encode()[:13] + b"\n" + b" " * 13)[:13])
    hfreq = (pclk // htotal) // 1000
    blob += desc(0xFD, bytes([
        max(1, r - 15), r + 1,
        min(30, max(1, hfreq - 20)), hfreq + 20,
        max(1, min(255, -(-pclk // 10_000_000))), 0x01, 0x0A,
        0x20, 0x20, 0x20, 0x20, 0x20, 0x20]))
    blob += bytes([0, 0, 0, 0x10, 0]) + bytes(13)  # dummy descriptor
    assert len(blob) == 126, len(blob)

    blob += bytes([1 if hdmi_mhz else 0])          # extensions
    e = bytearray(blob)
    e.append((256 - (sum(e) % 256)) % 256)         # checksum
    assert len(e) == 128 and sum(e) % 256 == 0, len(e)

    if hdmi_mhz:
        e += cta_with_hdmi_vsdb(hdmi_mhz)
        assert len(e) == 256

    with open(out, "wb") as f:
        f.write(e)
    print(f"{out}: {w}x{h}@{r} pclk={pclk/1e6:.2f}MHz hdmi_vsdb={hdmi_mhz or 'none'} "
          f"htotal={htotal} vtotal={vtotal} {mm[0]}x{mm[1]}mm "
          f"checksum_ok={sum(e) % 256 == 0}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

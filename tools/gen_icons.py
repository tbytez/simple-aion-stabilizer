#!/usr/bin/env python3
"""Rasterize assets/brand.svg into the native icon set.

brand.svg is the single vector source for the app identity mark: a
cyan-to-indigo gradient ring with a stroked "A". This script renders the
mark at the sizes each surface needs and writes:

  icon.png              window icon (embedded by src/main.rs via eframe)
  icon.ico              exe + taskbar icon (embedded by build.rs/app.rc)
  assets/tray_on.rgba   64x64 raw RGBA tray icon, engine running
  assets/tray_off.rgba  64x64 raw RGBA tray icon, engine stopped

The mark always sits on a dark circular badge so the white A keeps its
contrast on light taskbars and light tray themes. Rasterization is plain
supersampling - no third-party packages, deterministic output.

Tray status follows the in-app semantics (src/ui.rs): a running engine
turns the ring emerald; stopped keeps a muted slate mark.
"""

import math
import re
import struct
import sys
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SVG_PATH = ROOT / "assets" / "brand.svg"

# Dark card surface (theme.rs Palette::dark().card) as the badge fill.
BADGE = (19, 24, 36, 255)
# Mark colors matching the app theme and the approved reference.
A_WHITE = (242, 244, 249, 255)  # Palette::dark().text
RING_ON = ((52, 211, 153), (5, 150, 105))  # running: emerald gradient
RING_OFF = ((138, 147, 166), (95, 104, 120))  # stopped: muted slate

ICO_SIZES = [16, 24, 32, 48, 64, 256]


def hex_rgb(s):
    s = s.lstrip("#")
    return tuple(int(s[i : i + 2], 16) for i in (0, 2, 4))


def parse_mark(path):
    """Pull the exact mark geometry out of brand.svg (the known subset)."""
    svg = path.read_text()
    vb = re.search(r'viewBox="([^"]+)"', svg).group(1).split()
    grid = float(vb[2])
    stops = sorted(
        (float(off), hex_rgb(col))
        for off, col in re.findall(
            r'<stop[^>]*offset="([^"]+)"[^>]*stop-color="([^"]+)"', svg
        )
    )
    if not stops:
        stops = sorted(
            (float(off), hex_rgb(col))
            for off, col in re.findall(
                r'<stop[^>]*stop-color="([^"]+)"[^>]*offset="([^"]+)"', svg
            )
        )
    circle = re.search(
        r'<circle[^>]*cx="([^"]+)"[^>]*cy="([^"]+)"[^>]*r="([^"]+)"'
        r'[^>]*stroke-width="([^"]+)"',
        svg,
    )
    d = re.search(r'<path[^>]*d="([^"]+)"', svg).group(1)
    path_tag = re.search(r"<path[^>]*>", svg).group(0)
    a_w = float(re.search(r'stroke-width="([^"]+)"', path_tag).group(1))
    return {
        "grid": grid,
        "stops": stops,
        "cx": float(circle.group(1)),
        "cy": float(circle.group(2)),
        "r": float(circle.group(3)),
        "ring_w": float(circle.group(4)),
        "subpaths": parse_path(d),
        "a_w": a_w,
    }


def parse_path(d):
    """Subset of SVG path: M/m, L/l, H/h, and implicit lineto pairs."""
    tokens = re.findall(r"[MmLlHh]|-?[\d.]+", d)
    subpaths, cur, i, cmd, fresh = [], None, 0, None, True
    while i < len(tokens):
        if re.fullmatch(r"[MmLlHh]", tokens[i]):
            cmd = tokens[i]
            i += 1
            if cmd in "Mm":
                fresh = True  # a moveto starts a new subpath
            continue
        if cmd in "MmLl":
            x, y = float(tokens[i]), float(tokens[i + 1])
            i += 2
            if cmd.islower() and cur is not None:
                x, y = cur[0] + x, cur[1] + y
            cur = (x, y)
            if fresh or not subpaths:
                subpaths.append([])
                fresh = False
            subpaths[-1].append(cur)
            if cmd in "Mm":
                cmd = "l" if cmd == "m" else "L"  # implicit lineto follows
        elif cmd in "Hh":
            x = float(tokens[i])
            i += 1
            if cmd == "h" and cur is not None:
                x += cur[0]
            cur = (x, cur[1])
            subpaths[-1].append(cur)
        else:
            raise ValueError(f"unsupported path command {cmd!r} in {d!r}")
    return subpaths


def dist_seg(px, py, ax, ay, bx, by):
    vx, vy = bx - ax, by - ay
    wx, wy = px - ax, py - ay
    n = vx * vx + vy * vy
    t = 0.0 if n == 0 else max(0.0, min(1.0, (wx * vx + wy * vy) / n))
    dx, dy = wx - t * vx, wy - t * vy
    return math.hypot(dx, dy)


def render(mark, size, stops, glyph_rgb, badge_rgb, boost=1.0):
    """Supersampled raster of the mark -> bytearray RGBA (straight alpha)."""
    grid = mark["grid"]
    scale = size / grid
    ss = 8 if size <= 64 else 4
    cx, cy, r = mark["cx"], mark["cy"], mark["r"]
    ring_w = mark["ring_w"] * boost
    a_w = mark["a_w"] * boost
    ring_in, ring_out = r - ring_w / 2, r + ring_w / 2
    badge_r = ring_out  # badge fills the circle silhouette under the ring
    segs = [
        (a, b)
        for path in mark["subpaths"]
        for a, b in zip(path, path[1:])
    ]
    lo, hi = stops[0][1], stops[-1][1]
    top, span = cy - r, 2 * r
    out = bytearray(size * size * 4)
    for y in range(size):
        for x in range(size):
            ar = ag = ab = aa = 0.0
            for sy in range(ss):
                gy = (y + (sy + 0.5) / ss) / scale
                for sx in range(ss):
                    gx = (x + (sx + 0.5) / ss) / scale
                    dc = math.hypot(gx - cx, gy - cy)
                    px = None
                    if any(
                        dist_seg(gx, gy, ax, ay, bx, by) <= a_w / 2
                        for (ax, ay), (bx, by) in segs
                    ):
                        px = (*glyph_rgb, 255)
                    elif ring_in <= dc <= ring_out:
                        t = (gy - top) / span
                        px = tuple(
                            round(lo[c] + (hi[c] - lo[c]) * t) for c in range(3)
                        ) + (255,)
                    elif badge_rgb and dc <= badge_r:
                        px = badge_rgb
                    if px:
                        k = px[3] / 255.0
                        ar += px[0] * k
                        ag += px[1] * k
                        ab += px[2] * k
                        aa += px[3]
            n = ss * ss
            i = (y * size + x) * 4
            a = aa / n
            if a > 0:
                out[i] = round(ar * 255 / aa)
                out[i + 1] = round(ag * 255 / aa)
                out[i + 2] = round(ab * 255 / aa)
                out[i + 3] = round(aa / n)
    return out


def png_bytes(size, rgba):
    raw = b"".join(
        b"\x00" + bytes(rgba[y * size * 4 : (y + 1) * size * 4])
        for y in range(size)
    )

    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def write_ico(path, frames):
    header = struct.pack("<HHH", 0, 1, len(frames))
    entries, blobs, off = b"", b"", 6 + 16 * len(frames)
    for size, png in frames:
        w = 0 if size >= 256 else size
        entries += struct.pack(
            "<BBBBHHII", w, w, 0, 0, 1, 32, len(png), off
        )
        blobs += png
        off += len(png)
    Path(path).write_bytes(header + entries + blobs)


def main():
    mark = parse_mark(SVG_PATH)

    def boost_for(size):
        # Hairline strokes survive small frames with a slight weight boost.
        return 1.25 if size <= 24 else 1.0

    # Window/taskbar/exe identity: full mark on the dark badge.
    (ROOT / "icon.png").write_bytes(
        png_bytes(256, render(mark, 256, mark["stops"], A_WHITE[:3], BADGE))
    )
    write_ico(
        ROOT / "icon.ico",
        [
            (
                s,
                png_bytes(
                    s,
                    render(
                        mark,
                        s,
                        mark["stops"],
                        A_WHITE[:3],
                        BADGE,
                        boost=boost_for(s),
                    ),
                ),
            )
            for s in ICO_SIZES
        ],
    )

    # Tray: same mark, status shown by ring/glyph color like the app.
    for name, stops, glyph in [
        ("tray_on", RING_ON, A_WHITE[:3]),
        ("tray_off", RING_OFF, RING_OFF[0]),
    ]:
        rgba = render(mark, 64, [(0.0, stops[0]), (1.0, stops[1])], glyph, BADGE)
        (ROOT / "assets" / f"{name}.rgba").write_bytes(bytes(rgba))

    print("wrote icon.png (256), icon.ico (16/24/32/48/64/256),")
    print("assets/tray_on.rgba + assets/tray_off.rgba (64x64 RGBA)")


if __name__ == "__main__":
    sys.exit(main())

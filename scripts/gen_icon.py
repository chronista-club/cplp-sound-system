#!/usr/bin/env python3
"""CPLP Sound System — Halo アイコン生成器 (確定デザイン)"""

import json
import math
import os
import subprocess
import sys


def seeded_random(seed: int) -> float:
    x = math.sin(seed * 127.1 + 311.7) * 43758.5453
    return x - math.floor(x)


def generate_halo_svg(params: dict) -> str:
    p = params
    S = 1024
    cx = cy = 512
    margin = 30
    maxR = cx - margin
    nPoints = 180

    outerR = maxR * p["outerR"]
    innerR = outerR * (1 - p["ringWidth"])
    midR = (outerR + innerR) / 2
    bandHalf = (outerR - innerR) / 2

    svg_parts = []

    # Header + defs
    svg_parts.append(f'''<svg xmlns="http://www.w3.org/2000/svg" width="{S}" height="{S}" viewBox="0 0 {S} {S}">
  <defs>
    <radialGradient id="bg-grad" cx="50%" cy="50%" r="50%">
      <stop offset="0%" stop-color="#1a1a3e"/>
      <stop offset="100%" stop-color="{p["bgColor"]}"/>
    </radialGradient>
    <filter id="glow" x="-50%" y="-50%" width="200%" height="200%">
      <feGaussianBlur stdDeviation="8" result="blur"/>
      <feComposite in="SourceGraphic" in2="blur" operator="over"/>
    </filter>
    <filter id="glow-strong" x="-50%" y="-50%" width="200%" height="200%">
      <feGaussianBlur stdDeviation="16" result="blur"/>
      <feComposite in="SourceGraphic" in2="blur" operator="over"/>
    </filter>
    <filter id="glow-soft" x="-50%" y="-50%" width="200%" height="200%">
      <feGaussianBlur stdDeviation="4" result="blur"/>
      <feComposite in="SourceGraphic" in2="blur" operator="over"/>
    </filter>
    <radialGradient id="embrace-grad" cx="50%" cy="50%" r="50%">
      <stop offset="0%" stop-color="{p["pearlCenter"]}" stop-opacity="1"/>
      <stop offset="40%" stop-color="#ffccd5" stop-opacity="0.6"/>
      <stop offset="100%" stop-color="{p["pearlEdge"]}" stop-opacity="0"/>
    </radialGradient>
    <radialGradient id="pearl-grad" cx="38%" cy="33%" r="65%">
      <stop offset="0%" stop-color="{p["pearlCenter"]}"/>
      <stop offset="25%" stop-color="#ffccd5"/>
      <stop offset="55%" stop-color="#ff4d6d"/>
      <stop offset="100%" stop-color="{p["pearlEdge"]}"/>
    </radialGradient>
  </defs>''')

    # Background
    svg_parts.append(f'  <rect width="{S}" height="{S}" rx="220" fill="url(#bg-grad)"/>')

    # Stars
    for si in range(p["starCount"]):
        sAngle = seeded_random(si * 3 + 1) * math.pi * 2
        sR = maxR * (0.15 + p["starSpread"] * seeded_random(si * 3 + 2))
        sx = cx + sR * math.cos(sAngle)
        sy = cy + sR * math.sin(sAngle)
        sSize = p["starSize"] * (0.5 + seeded_random(si * 3 + 3) * 0.8)
        sOp = 0.3 + seeded_random(si * 7) * 0.6

        pts = []
        for sp in range(8):
            sa = sp * math.pi / 4 - math.pi / 8
            sr = sSize if sp % 2 == 0 else sSize * 0.35
            pts.append(f"{sx + sr * math.cos(sa):.1f},{sy + sr * math.sin(sa):.1f}")
        star_d = "M " + " L ".join(pts) + " Z"
        svg_parts.append(f'  <path d="{star_d}" fill="{p["starColor"]}" opacity="{sOp:.2f}" filter="url(#glow-soft)"/>')

    # Halo ring base
    svg_parts.append(
        f'  <circle cx="{cx}" cy="{cy}" r="{midR:.0f}" fill="none" stroke="{p["colors"][0]}" '
        f'stroke-width="{bandHalf * 2:.0f}" opacity="{p["ringOp"]}"/>'
    )

    # 3 Lanes
    sweepRad = p["sweep"] * math.pi / 180
    gapAngle = 2 * math.pi / 3

    for a in range(3):
        startAngle = a * gapAngle - math.pi / 2
        color = p["colors"][a % len(p["colors"])]

        outer_edge = []
        inner_edge = []

        for i in range(nPoints):
            t = i / (nPoints - 1)
            angle = startAngle + t * sweepRad
            armW = bandHalf * p["laneWidth"] / 0.28 * (1 - t * p["taper"])
            wb = p["wobble"] * math.sin(p["wobbleFreq"] * t * math.pi * 2)

            rOuter = midR + armW + wb
            rInner = midR - armW + wb * 0.5

            outer_edge.append((cx + rOuter * math.cos(angle), cy + rOuter * math.sin(angle)))
            inner_edge.append((cx + max(0, rInner) * math.cos(angle), cy + max(0, rInner) * math.sin(angle)))

        outer_str = " L ".join(f"{x:.1f},{y:.1f}" for x, y in outer_edge)
        inner_str = " L ".join(f"{x:.1f},{y:.1f}" for x, y in reversed(inner_edge))
        d = f"M {outer_str} L {inner_str} Z"

        svg_parts.append(f'  <path d="{d}" fill="{color}" opacity="{p["laneOp"]}" filter="url(#glow)"/>')

    # Diamonds
    for di in range(p["diaCount"]):
        dAngle = seeded_random(di * 5 + 100) * math.pi * 2
        dR = maxR * (0.2 + p["diaSpread"] * seeded_random(di * 5 + 101))
        dx = cx + dR * math.cos(dAngle)
        dy = cy + dR * math.sin(dAngle)
        dSize = p["diaSize"] * (0.5 + seeded_random(di * 5 + 102) * 0.7)
        dOp = 0.25 + seeded_random(di * 5 + 103) * 0.5
        dRotate = seeded_random(di * 5 + 104) * 45

        pts = []
        for dp in range(4):
            da = dp * math.pi / 2 + dRotate * math.pi / 180
            dr = dSize if dp % 2 == 0 else dSize * 0.6
            pts.append(f"{dx + dr * math.cos(da):.1f},{dy + dr * math.sin(da):.1f}")
        dia_d = "M " + " L ".join(pts) + " Z"
        svg_parts.append(f'  <path d="{dia_d}" fill="{p["diaColor"]}" opacity="{dOp:.2f}" filter="url(#glow-soft)"/>')

    # Embrace
    embraceR = maxR * p["embrace"]
    svg_parts.append(
        f'  <circle cx="{cx}" cy="{cy}" r="{embraceR:.0f}" fill="url(#embrace-grad)" '
        f'opacity="{p["embraceOp"]}" filter="url(#glow-strong)"/>'
    )

    # Pearl
    pr = maxR * p["pearlSize"]
    svg_parts.append(f'  <circle cx="{cx}" cy="{cy}" r="{pr:.0f}" fill="url(#pearl-grad)" opacity="0.95" filter="url(#glow-strong)"/>')
    svg_parts.append(f'  <circle cx="{cx}" cy="{cy}" r="{pr * 0.4:.0f}" fill="{p["pearlCenter"]}" opacity="0.3"/>')
    svg_parts.append(f'  <circle cx="{cx}" cy="{cy}" r="5" fill="white" opacity="0.9"/>')

    svg_parts.append("</svg>")
    return "\n".join(svg_parts)


def svg_to_png(svg_path: str, png_path: str, size: int):
    subprocess.run([
        "rsvg-convert", "-w", str(size), "-h", str(size), svg_path, "-o", png_path
    ], check=True)


def create_icns(png_1024: str, output_dir: str):
    """1024px PNG から macOS .icns を生成"""
    iconset = os.path.join(output_dir, "AppIcon.iconset")
    os.makedirs(iconset, exist_ok=True)

    sizes = [16, 32, 64, 128, 256, 512, 1024]
    for s in sizes:
        # 1x
        out = os.path.join(iconset, f"icon_{s}x{s}.png")
        subprocess.run(["sips", "-z", str(s), str(s), png_1024, "--out", out],
                       check=True, capture_output=True)
        # 2x (half the name)
        if s >= 32:
            half = s // 2
            out2x = os.path.join(iconset, f"icon_{half}x{half}@2x.png")
            subprocess.run(["sips", "-z", str(s), str(s), png_1024, "--out", out2x],
                           check=True, capture_output=True)

    icns_path = os.path.join(output_dir, "AppIcon.icns")
    subprocess.run(["iconutil", "-c", "icns", iconset, "-o", icns_path], check=True)
    print(f"Created: {icns_path}")

    # Cleanup iconset
    import shutil
    shutil.rmtree(iconset)


def create_appiconset(png_1024: str, output_dir: str):
    """iOS/macOS AppIcon.appiconset を生成"""
    appiconset = os.path.join(output_dir, "AppIcon.appiconset")
    os.makedirs(appiconset, exist_ok=True)

    # iOS + macOS の全サイズ
    icon_sizes = [
        (1024, "appicon_1024.png"),
        (180, "appicon_180.png"),   # iPhone @3x
        (120, "appicon_120.png"),   # iPhone @2x
        (167, "appicon_167.png"),   # iPad Pro @2x
        (152, "appicon_152.png"),   # iPad @2x
        (76, "appicon_76.png"),    # iPad @1x
        (512, "appicon_512.png"),   # macOS 256@2x
        (256, "appicon_256.png"),   # macOS 128@2x
        (128, "appicon_128.png"),   # macOS
        (64, "appicon_64.png"),    # macOS 32@2x
        (32, "appicon_32.png"),    # macOS
        (16, "appicon_16.png"),    # macOS
    ]

    for size, name in icon_sizes:
        out = os.path.join(appiconset, name)
        subprocess.run(["sips", "-z", str(size), str(size), png_1024, "--out", out],
                       check=True, capture_output=True)

    # Contents.json
    contents = {
        "images": [{"filename": name, "idiom": "universal", "platform": "ios", "size": f"{s}x{s}"}
                    for s, name in icon_sizes[:1]] +
                  [{"filename": name, "idiom": "mac", "size": f"{s}x{s}"}
                    for s, name in icon_sizes[6:]],
        "info": {"author": "cplp-gen-icon", "version": 1}
    }
    with open(os.path.join(appiconset, "Contents.json"), "w") as f:
        json.dump(contents, f, indent=2)

    print(f"Created: {appiconset}")


if __name__ == "__main__":
    # パラメータ読み込み
    params_path = os.path.join(os.path.dirname(__file__), "..", "assets", "icons", "halo_params.json")
    with open(params_path) as f:
        params = json.load(f)

    out_dir = "assets/icons"
    os.makedirs(out_dir, exist_ok=True)

    # SVG 生成
    svg_content = generate_halo_svg(params)
    svg_path = os.path.join(out_dir, "cplp_halo.svg")
    with open(svg_path, "w") as f:
        f.write(svg_content)
    print(f"Generated: {svg_path}")

    # PNG 生成
    png_path = os.path.join(out_dir, "cplp_halo_1024.png")
    svg_to_png(svg_path, png_path, 1024)
    print(f"Generated: {png_path}")

    # macOS .icns
    create_icns(png_path, out_dir)

    # Xcode AppIcon.appiconset
    xcassets_dir = "apple/CplpSoundSystem/Resources/Assets.xcassets"
    create_appiconset(png_path, xcassets_dir)

    print("\nDone!")

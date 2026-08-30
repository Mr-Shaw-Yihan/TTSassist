#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""磨砂金图标生成器（设计文档 §4.3/§4.4，v3 黑金改版）。

实心图标（_f 系列，主遥控页黑色按钮面用）：金渐变 + 高密度噪斑（磨砂颗粒，
flutter_svg 不支持 feTurbulence，用固定种子随机小方点模拟）+ clipPath 裁进图形。
mic_off 的斜杠用黑钮底色 #17150F 覆盖实现「挖空」，仅在黑色按钮面上使用。
描边旧版仅保留 empty_fav（毛玻璃空态插图）。

用法：python tools/gen_icons.py  （产物写入 assets/icons/）
改图形后重跑即可，源码进 git、产物可随时再生成。
"""

import os
import random
import struct
import zlib

GOLD_HI = "#E9CA6B"
GOLD_LO = "#C69A2E"
TILE = "#17150F"  # 主遥控页黑色按钮面（mic_off 斜杠覆盖色须与其一致）
INK = "#1A1816"
SUB = "#6B675F"
OUT_DIR = os.path.join(os.path.dirname(__file__), "..", "assets", "icons")

rng = random.Random(20260824)  # 固定种子：产物可复现


def grain(count=900):
    """实心图标内部噪斑：细粒、中高对比（对齐 v3 预览的磨砂观感）"""
    parts = []
    for _ in range(count):
        x = rng.uniform(0, 48)
        y = rng.uniform(0, 48)
        s = rng.uniform(0.5, 1.3)
        white = rng.random() < 0.5
        opacity = rng.uniform(0.16, 0.38) if white else rng.uniform(0.14, 0.30)
        color = "#FFF1C4" if white else "#4A3608"
        parts.append(
            f'<rect x="{x:.1f}" y="{y:.1f}" width="{s:.1f}" height="{s:.1f}" '
            f'fill="{color}" opacity="{opacity:.2f}"/>'
        )
    return "".join(parts)


def filled_svg(shapes, slash=None):
    """实心磨砂金图标：金渐变底 + 噪斑，一起裁进形状；slash 以黑钮底色覆盖（限黑底使用）"""
    out = (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48">'
        "<defs>"
        '<linearGradient id="gg" x1="0" y1="0" x2="0" y2="1">'
        f'<stop offset="0" stop-color="{GOLD_HI}"/>'
        f'<stop offset="1" stop-color="{GOLD_LO}"/>'
        "</linearGradient>"
        f"<clipPath id=\"ic\">{shapes}</clipPath>"
        "</defs>"
        '<g clip-path="url(#ic)">'
        '<rect width="48" height="48" fill="url(#gg)"/>'
        + grain()
        + "</g>"
    )
    if slash:
        out += (
            f'<path d="{slash}" fill="none" stroke="{TILE}" '
            'stroke-width="4.6" stroke-linecap="round"/>'
        )
    return out + "</svg>"


def mic_fill():
    """实心麦克风：胶囊 + 下半环支架 + 尾杆 + 底座"""
    return (
        '<rect x="15.5" y="5" width="17" height="20" rx="8.5"/>'
        '<path d="M13 23 A11 11 0 0 0 35 23 L31 23 A7 7 0 0 1 17 23 Z"/>'
        '<rect x="21.8" y="34" width="4.4" height="5.2" rx="1"/>'
        '<rect x="15.5" y="40.4" width="17" height="3.4" rx="1.7"/>'
    )


ICONS = {
    # 麦克风开（黑钮面用）
    "mic_on_f": filled_svg(mic_fill()),
    # 麦克风关：同形 + 左上到右下斜杠（黑钮底色覆盖）
    "mic_off_f": filled_svg(mic_fill(), slash="M9 7 L41 41"),
    # 播放上一条：竖条 + 左向实心三角
    "play_last_f": filled_svg(
        '<rect x="13" y="10.5" width="4.6" height="27" rx="1"/>'
        '<path d="M37 11.8 V36.2 c0 1.35 -1.52 2.14 -2.62 1.36 '
        'L18.1 26.1 c-1.47 -1.04 -1.47 -3.22 0 -4.26 L34.38 10.44 '
        'c1.1 -0.78 2.62 0.01 2.62 1.36 Z"/>'
    ),
    # 发送：实心纸飞机（毛玻璃输入栏用）
    "send_f": filled_svg(
        '<path d="M42.7 5.6 L4.3 21.2 c-1.7 .7 -1.6 3.1 .2 3.7 l10.6 3.4 '
        '3.9 12.4 c.5 1.7 2.8 2 3.8 .5 l5.3 -7.6 9.6 7.1 c1.3 1 3.2 .3 '
        '3.6 -1.3 l4.3 -30.5 c.3 -1.9 -1.7 -3.4 -3.4 -2.6 Z"/>'
    ),
    # 跳转顶部：实心上箭头
    "jump_top_f": filled_svg(
        '<path d="M24 9.5 L37.8 23.3 l-3.9 3.9 -7.2 -7.2 V38.5 h-5.4 V20 '
        'l-7.2 7.2 -3.9 -3.9 Z"/>'
    ),
    # 跳转底部：实心下箭头
    "jump_bottom_f": filled_svg(
        '<path d="M24 38.5 L10.2 24.7 l3.9 -3.9 7.2 7.2 V9.5 h5.4 V28 '
        'l7.2 -7.2 3.9 3.9 Z"/>'
    ),
    # 收藏空态插图：星星 + 引导点（毛玻璃空态卡，描边灰）
    "empty_fav": (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48">'
        f'<g fill="none" stroke="{SUB}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">'
        '<path d="M24 10 l4.2 8.5 9.4 1.4 -6.8 6.6 1.6 9.3 -8.4 -4.4 -8.4 4.4 1.6 -9.3 -6.8 -6.6 9.4 -1.4 Z"/>'
        '<path d="M8 42 h32" stroke-dasharray="2 4"/>'
        "</g></svg>"
    ),
}


def _png_chunk(tag, data):
    c = struct.pack(">I", len(data)) + tag + data
    return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)


def gen_texture(path, size=96, count=1100):
    """透明噪点瓦片（白/深棕半透明颗粒），Flutter 侧作磨砂金的 foreground 叠层"""
    px = bytearray(size * size * 4)
    for _ in range(count):
        x = rng.randint(0, size - 1)
        y = rng.randint(0, size - 1)
        s = rng.choice((1, 1, 2))
        white = rng.random() < 0.45
        a = rng.randint(14, 42) if white else rng.randint(10, 30)
        col = bytes((255, 255, 255, a) if white else (61, 46, 8, a))
        for dy in range(s):
            for dx in range(s):
                i = (((y + dy) % size) * size + ((x + dx) % size)) * 4
                px[i:i + 4] = col
    raw = b"".join(
        b"\x00" + bytes(px[y * size * 4:(y + 1) * size * 4]) for y in range(size)
    )
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    data = (
        b"\x89PNG\r\n\x1a\n"
        + _png_chunk(b"IHDR", ihdr)
        + _png_chunk(b"IDAT", zlib.compress(raw, 9))
        + _png_chunk(b"IEND", b"")
    )
    with open(path, "wb") as f:
        f.write(data)
    print("生成", os.path.relpath(path, os.path.join(OUT_DIR, "..", "..")))


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    for name, content in ICONS.items():
        path = os.path.join(OUT_DIR, f"{name}.svg")
        with open(path, "w", encoding="utf-8") as f:
            f.write(content)
        print("生成", os.path.relpath(path, os.path.join(OUT_DIR, "..", "..")))
    gen_texture(os.path.join(OUT_DIR, "gold_texture.png"))
    print(f"共 {len(ICONS)} 个图标 + 1 个噪点瓦片")


if __name__ == "__main__":
    main()

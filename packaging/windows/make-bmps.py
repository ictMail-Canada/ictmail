#!/usr/bin/env python3
"""Regenerate the WiX installer bitmaps in this folder.

WiX shows two images, both 493xN here for hi-DPI (shown at 2/3 size):

  banner.bmp  the strip at the top of most dialogs (shown 370x44). The dialog
              titles sit on it, so it stays pale; the mark sits at the right edge.
  dialog.bmp  the big picture on the welcome and finish pages (shown 370x234).
              Its left third is the navy brand panel with the app mark; the rest
              is pale, because the page titles and text are drawn over it.

The app mark is assets/branding/app-Icon.png. Run from the repo root:

    python packaging/windows/make-bmps.py
"""

from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
ASSETS = Path(__file__).resolve().parent / "assets"
MARK = ROOT / "assets" / "branding" / "app-Icon.png"

# The dialog text starts at x=135 of 370 shown px; the bitmap is 493 px for those
# 370, so the pale zone must begin well left of 135 * 493 / 370 ~= 180.
PANEL_W = 172  # navy brand panel width in dialog.bmp

NAVY_TOP = (4, 22, 66)       # #041642
NAVY_BOT = (8, 42, 92)       # #082A5C
PALE = (242, 248, 253)       # #F2F8FD
ACCENT = (55, 190, 255)      # #37BEFF, the brand blue
BANNER_L = (234, 244, 252)   # #EAF4FC
BANNER_R = (208, 230, 244)   # #D0E6F4


def vgrad(size, top, bottom):
    """A vertical gradient image of `size` ((w, h), c1, c2)."""
    w, h = size
    img = Image.new("RGB", size)
    px = img.load()
    for y in range(h):
        t = y / max(h - 1, 1)
        c = tuple(round(top[i] + (bottom[i] - top[i]) * t) for i in range(3))
        for x in range(w):
            px[x, y] = c
    return img


def hgrad(size, left, right):
    w, h = size
    img = Image.new("RGB", size)
    px = img.load()
    for x in range(w):
        t = x / max(w - 1, 1)
        c = tuple(round(left[i] + (right[i] - left[i]) * t) for i in range(3))
        for y in range(h):
            px[x, y] = c
    return img


def mark(size):
    """The app mark scaled to `size` ((w, h)), kept transparent."""
    icon = Image.open(MARK).convert("RGBA")
    icon.thumbnail(size, Image.LANCZOS)
    return icon


def banner():
    img = hgrad((493, 58), BANNER_L, BANNER_R)
    # The mark at the right edge, vertically centred.
    m = mark((44, 44))
    img.paste(m, (493 - 8 - m.width, (58 - m.height) // 2), m)
    # A hairline of brand blue along the bottom, as on dialog.bmp.
    for y in range(55, 58):
        for x in range(493):
            img.putpixel((x, y), ACCENT)
    img.save(ASSETS / "banner.bmp")


def dialog():
    img = Image.new("RGB", (493, 312), PALE)
    # Brand panel on the left, and the pale text zone on the right of it.
    img.paste(vgrad((PANEL_W, 312), NAVY_TOP, NAVY_BOT), (0, 0))
    # Brand-blue rule between the panel and the text zone.
    for y in range(312):
        for x in range(PANEL_W, PANEL_W + 3):
            img.putpixel((x, y), ACCENT)
    # The mark, centred in the panel.
    m = mark((116, 116))
    img.paste(m, ((PANEL_W - m.width) // 2, (312 - m.height) // 2), m)
    # The same hairline along the bottom.
    for y in range(308, 312):
        for x in range(493):
            img.putpixel((x, y), ACCENT)
    img.save(ASSETS / "dialog.bmp")


if __name__ == "__main__":
    banner()
    dialog()
    print("wrote banner.bmp and dialog.bmp")

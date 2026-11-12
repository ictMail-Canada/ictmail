# ictMail app icon

The **ictMail envelope mark** — the orange-rimmed, blue-filled envelope of the ictMail logo —
on transparency, the same mark the favicon uses.

The mark is ictTech's own app-icon artwork (`assets/branding/app-Icon.png`), kept content-cropped
as `source/ictmail-mark.png` and embedded in `source/mailcraft.svg` as a PNG image. It is
ictTech's own work under the project's licence (MIT OR Apache-2.0). The `alternates/` drawings
are the upstream MailCraft icon in other colour schemes, kept for reference and not used.

## Files

| Path | What |
|---|---|
| `source/mailcraft.svg` | The source: everything else here is written from it |
| `source/ictmail-mark.png` | The mark itself, content-cropped from `../branding/app-Icon.png` |
| `alternates/calmail-{teal,violet,sunset,green,graphite}.svg` | The same drawing in other colour schemes, kept for the future and not used yet |
| `alternates/gen.py` | Writes every scheme, blue included, from one template and a palette per scheme |
| `hicolor/` | Linux: PNGs from 16 to 512 px, and the SVG as the scalable icon |
| `mailcraft.ico` | Windows: 16 to 256 px |
| `mailcraft.icns`, `mailcraft-macos-512.png` | macOS, on Apple's 824/1024 grid |
| `mailcraft-1024.png` | The icon at 1024 px |

## Geometry

The drawing is fitted to the canvas, whatever margin the SVG keeps round it (the bounding box
of what it draws, made square about its centre); the scalable icon's `viewBox` is set to the
same square. The macOS files keep Apple's margin (an 824 px body on a 1024 px canvas). The 16,
24, 32 and 48 px files show the drawing a little closer inside the same bounds (1.14, 1.08,
1.08 and 1.04 times), so the envelope still reads.

## Changing it

Edit `source/mailcraft.svg` (or the template in `alternates/gen.py`, then copy its blue output
there), then:

```sh
cargo xtask icon
```

It renders the SVG with `resvg`, a pure-Rust renderer that only xtask depends on, and writes every
file above. Commit what changed; each file has its row in `ATTRIBUTION.md`.

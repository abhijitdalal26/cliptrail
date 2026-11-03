# ClipTrail brand assets

Source artwork for the ClipTrail icon and wordmark. The rendered platform
files (Tauri icons, `.icns`, `.ico`, favicons, Android mipmaps) are generated
from these vectors.

## Files

Icon (the squircle mark, white clipboard with a history clock, #2F93FD to #8A2AFB):

- `cliptrail-icon.svg` / `.png` — the canonical app icon (1024×1024).
- `cliptrail-icon-fullbleed.svg` / `.png` — same artwork without the squircle
  mask, edge to edge; for platforms that apply their own mask (Android
  adaptive background, store listings).
- `cliptrail-icon-foreground.svg` / `.png` — glyph only (transparent), for
  compositing: adaptive-icon foreground, tray, watermarking.
- `cliptrail-icon-mono-dark.svg` / `.png` and `cliptrail-icon-mono-light.svg` /
  `.png` — single-color silhouettes (dark ink / white). The alpha channel is
  the mark; use for template/monochrome contexts (macOS menu-bar icon,
  Android 13+ themed icon, badges).

Logo (icon + "ClipTrail" wordmark, Outfit Bold outlined as paths):

- `cliptrail-logo.svg` / `.png` — full-color, for light backgrounds.
- `cliptrail-logo-mono-dark.svg` / `.png` — single-color dark ink, for light
  backgrounds where color is not available (print, stamps).
- `cliptrail-logo-mono-light.svg` / `.png` — single-color white, for dark
  backgrounds (used by the README in dark mode).

Platform sets (rendered from the vectors above):

- `android/` — launcher icons: `res/mipmap-*` (legacy `ic_launcher`,
  `ic_launcher_round`, adaptive `ic_launcher_foreground`; the
  `ic_launcher_background.png` rasters are unused because the adaptive
  background is the `drawable/ic_launcher_background.xml` gradient),
  `res/mipmap-anydpi-v26` (adaptive icon XML), `res/drawable`
  (`ic_launcher_monochrome.png` for the Android 13+ themed icon) and
  `play-store-512.png`. Installed in
  `apps/desktop/src-tauri/gen/android/app/src/main/res/`.
- `linux/hicolor/` — freedesktop icon theme tree (16–512 px PNG + `scalable/`
  SVG), named `cliptrail`.
- `macos/cliptrail.icns`

SVGs and Android XML are stored with LF line endings.

## Where the rendered outputs live

| Surface | Location |
|---|---|
| Desktop app icon (Windows/macOS/Linux) | `apps/desktop/src-tauri/icons/` |
| Tray / menu-bar icon | `apps/desktop/src-tauri/icons/tray*.png` (from `cliptrail-icon-mono-dark`) |
| Site favicon + web icons | `site/favicon.svg`, `site/favicon.ico`, `site/assets/` |
| Site header/footer mark | inline `LOGO` in `site/_src/build.py` (simplified flat variant) |
| Social card | `site/assets/og.png` (regenerate via `site/_src/og.html`) |

When the mark changes, regenerate all of the above rather than editing the
rasters directly.

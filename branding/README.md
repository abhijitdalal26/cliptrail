# ClipTrail brand assets

Source artwork for the ClipTrail icon and wordmark. The rendered platform
files (Tauri icons, `.icns`, `.ico`, favicons, Android mipmaps) are generated
from these vectors.

## Files

- `cliptrail-icon.svg` — the app icon: white clipboard with a history clock on a
  blue-to-purple squircle (#2F93FD → #8A2AFB). This is the canonical mark.
- `cliptrail-icon.png` — 1024×1024 raster of the icon.
- `cliptrail-icon-foreground.svg` — glyph only, for compositing (e.g. tray,
  watermarking).
- `cliptrail-icon-mono-dark.svg` / `cliptrail-icon-mono-light.svg` — single-color
  silhouettes. The alpha channel is the mark; use for template/monochrome
  contexts (macOS menu-bar icon, badges).
- `cliptrail-logo.svg` / `cliptrail-logo.png` — icon + "ClipTrail" wordmark
  (Outfit typeface).
- `android/` — launcher icons (all `mipmap-*` densities, adaptive icon +
  monochrome, Play Store 512). Drop `res/` into `gen/android/app/src/main/`
  when the Android app is scaffolded.

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

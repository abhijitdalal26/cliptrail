# Linux notes for contributors

How capture, paste-back and desktop integration work per session type, and how
to verify each one for real. The matrix from the user's point of view lives in
the site docs (`site/_src/docs/troubleshooting.html`).

## Session types and what works

| | X11 | KDE Plasma 6 Wayland | Sway / Hyprland | GNOME Wayland |
|---|---|---|---|---|
| capture | full | full (data-control) | full (data-control) | X11 apps only* |
| paste-back | EWMH + XTest | copy-only | copy-only | copy-only |
| global hotkey | yes | bind DE shortcut* | bind DE shortcut* | bind DE shortcut* |

\* details below.

**GNOME Wayland** has no `zwlr_data_control_manager_v1` and never will (GNOME
policy). `clipd-capture` detects this and falls back to the X11 backend, so it
sees copies made while an XWayland window has focus — native Wayland windows
are invisible to it. There is no permitted alternative; this is documented to
users as a limitation, not worked around.

**Global shortcut on Wayland:** there is no app-facing global shortcut protocol
in wlroots or KWin (the XDG GlobalShortcuts portal exists but is not widely
implemented). The app therefore supports `cliptrail --picker`: a second launch
forwards to the running instance via the single-instance plugin and opens the
picker. Users bind `cliptrail --picker` to a desktop shortcut
(KDE: System Settings → Shortcuts → Custom; GNOME: Settings → Keyboard →
Custom Shortcuts). On X11 the built-in `Ctrl+Shift+Space` works.

**Paste-back** (`crates/clipd-capture/src/linux/x11/paste.rs`): X11 only.
Activates the window that had focus before the picker via EWMH, then sends a
synthetic keystroke with XTest. Terminal windows are detected by `WM_CLASS`
(≈40 known classes, walking up to 16 ancestors for reparented windows) and get
`Ctrl+Shift+V` instead of `Ctrl+V`. On Wayland there is no way to type into
another window without root (`ydotool` needs a daemon running as root, portals
need a screencast-style dialog each time) — the picker falls back to writing
the clipboard and showing "Copied — press Ctrl+V to paste".

## Running the live tests

The live suite (`crates/clipd-capture/src/linux/live_tests.rs`) is `#[ignore]`d
— it needs a real session and is serial:

```sh
cargo test -p clipd-capture -- --ignored --test-threads=1
```

### X11 (real session or Xvfb)

```sh
sudo apt install xvfb xclip
xvfb-run -a -s "-screen 0 1280x800x24" \
    env -u WAYLAND_DISPLAY \
    cargo test -p clipd-capture -- --ignored --test-threads=1
```

Covers text, text+HTML, PNG images, file lists, 300 KB+ INCR transfers,
password-manager hint targets (`x-kde-passwordManagerHint` and friends are
skipped), burst debouncing, external-tool copies (xclip), write-back
round-trips, paste into a normal window vs a terminal (`WM_CLASS` →
Ctrl+Shift+V), and watcher stop/restart.

### Wayland data-control (headless sway)

Weston does **not** work for this — its headless build advertises only
`wl_data_device_manager` v3, no data-control. Sway does:

```sh
sudo apt install sway wl-clipboard
export XDG_RUNTIME_DIR=/tmp/xdg-sway WAYLAND_DISPLAY=wayland-1
mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman sway -c /dev/null &
# wait for the socket
for i in $(seq 50); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.2; done
cargo test -p clipd-capture -- --ignored --test-threads=1
kill %1
```

The same steps run in CI (`linux-live-clipboard` job in `.github/workflows/ci.yml`).

### Manual end-to-end checks

```sh
# capture: copy something, then
clipd list | head

# sync hub + pairing on one machine:
CLIPTRAIL_DATA_DIR=/tmp/hub clipd sync host --loopback
CLIPTRAIL_DATA_DIR=/tmp/client clipd sync pair 'cliptrail://pair?...'

# picker via single-instance handoff (desktop app running):
cliptrail --picker
```

## CI

`.github/workflows/ci.yml` → `linux-live-clipboard`: installs Xvfb, xclip,
wl-clipboard and sway, then runs the ignored tests once under Xvfb (X11) and
once under headless sway (data-control Wayland) with a socket poll instead of
sleeps.

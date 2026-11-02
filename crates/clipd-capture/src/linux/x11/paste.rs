//! Paste-back on X11: remember the active window, later refocus it and fake
//! Ctrl+V through the XTEST extension.
//!
//! Focus strategy:
//! 1. EWMH `_NET_ACTIVE_WINDOW` client message (source indication 2, "pager",
//!    so focus-stealing prevention lets it through), polled up to 150 ms.
//! 2. Fallback: `XSetInputFocus` + raise, polled up to 50 ms.
//!
//! Held modifiers (typically the picker hotkey) are released first so they do
//! not combine with our Ctrl+V. Terminal emulators get Ctrl+Shift+V instead:
//! for them Ctrl+V is "literal next", not paste.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt, EventMask, InputFocus,
    KEY_PRESS_EVENT, KEY_RELEASE_EVENT, StackMode, Window,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use super::proc::{active_window, pid_of_window_tree};
use super::{Atoms, connect};
use crate::ForeignWindow;

const POLL_STEP: Duration = Duration::from_millis(10);
const EWMH_WAIT: Duration = Duration::from_millis(150);
const FOCUS_WAIT: Duration = Duration::from_millis(50);

const XK_V: u32 = 0x0076;
const XK_CONTROL_L: u32 = 0xffe3;
const XK_SHIFT_L: u32 = 0xffe1;
/// Physical-key fallbacks (evdev + 8) if the keymap has no such keysym.
const KEYCODE_V_FALLBACK: u8 = 55;
const KEYCODE_CTRL_L_FALLBACK: u8 = 37;
const KEYCODE_SHIFT_L_FALLBACK: u8 = 50;

/// WM_CLASS classes whose paste shortcut is Ctrl+Shift+V (Ctrl+V is
/// "literal next" in a terminal). Lowercase, matched case-insensitively.
/// Editors with an integrated terminal (code, codium) are deliberately
/// left out: there Ctrl+V is right for the editor itself.
const TERMINAL_CLASSES: &[&str] = &[
    "xterm",
    "uxterm",
    "konsole",
    "gnome-terminal",
    "gnome-terminal-server",
    "kgx",
    "mate-terminal",
    "xfce4-terminal",
    "lxterminal",
    "tilix",
    "terminator",
    "alacritty",
    "kitty",
    "wezterm",
    "wezterm-gui",
    "foot",
    "st",
    "st-256color",
    "simpleterminal",
    "termite",
    "urxvt",
    "rxvt-unicode",
    "rxvt-unicode-256color",
    "eterm",
    "aterm",
    "mlterm",
    "sakura",
    "terminology",
    "qterminal",
    "yakuake",
    "guake",
    "tilda",
    "cool-retro-term",
    "deepin-terminal",
    "io.elementary.terminal",
    "hyper",
    "tabby",
    "contour",
    "blackbox",
];

fn is_terminal_class(class: &str) -> bool {
    let class = class.to_ascii_lowercase();
    TERMINAL_CLASSES.iter().any(|t| *t == class)
}

/// The WM_CLASS class part of `win` or the nearest ancestor that has one.
fn wm_class(conn: &RustConnection, atoms: &Atoms, root: Window, mut win: Window) -> Option<String> {
    for _ in 0..16 {
        if win == x11rb::NONE || win == root {
            return None;
        }
        if let Some(reply) = conn
            .get_property(false, win, atoms.WM_CLASS, AtomEnum::STRING, 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
            && !reply.value.is_empty()
        {
            // WM_CLASS is "instance\0class\0".
            let class = reply
                .value
                .split(|&b| b == 0)
                .rfind(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned());
            if let Some(class) = class {
                return Some(class);
            }
        }
        win = conn.query_tree(win).ok()?.reply().ok()?.parent;
    }
    None
}

fn session() -> Result<(RustConnection, Atoms, Window)> {
    let (conn, screen_num) = connect()?;
    let root = conn.setup().roots[screen_num].root;
    let atoms = Atoms::new(&conn)?.reply().context("interning atoms")?;
    Ok((conn, atoms, root))
}

/// See [`crate::remember_foreground`].
pub(in crate::linux) fn remember_foreground() -> Option<ForeignWindow> {
    let (conn, atoms, root) = session().ok()?;
    let win = active_window(&conn, &atoms, root).or_else(|| {
        let focus = conn.get_input_focus().ok()?.reply().ok()?.focus;
        // 0 = None, 1 = PointerRoot
        (focus > 1 && focus != root).then_some(focus)
    })?;
    // One of our own windows (the picker) is not a paste target.
    if pid_of_window_tree(&conn, &atoms, root, win) == Some(std::process::id()) {
        return None;
    }
    Some(ForeignWindow(win as isize))
}

/// `win` itself, or any descendant of it (focus often sits on a child).
fn in_tree(conn: &RustConnection, root: Window, win: Window, target: Window) -> bool {
    let mut cur = win;
    for _ in 0..16 {
        if cur == target {
            return true;
        }
        if cur <= 1 || cur == root {
            return false;
        }
        match conn.query_tree(cur).ok().and_then(|c| c.reply().ok()) {
            Some(t) => cur = t.parent,
            None => return false,
        }
    }
    false
}

fn is_active(conn: &RustConnection, atoms: &Atoms, root: Window, target: Window) -> bool {
    active_window(conn, atoms, root) == Some(target)
}

fn focus_in_target(conn: &RustConnection, root: Window, target: Window) -> bool {
    conn.get_input_focus()
        .ok()
        .and_then(|c| c.reply().ok())
        .is_some_and(|r| in_tree(conn, root, r.focus, target))
}

fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + limit;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(POLL_STEP);
    }
}

fn activate(conn: &RustConnection, atoms: &Atoms, root: Window, target: Window) -> Result<()> {
    // data: [source indication (2 = pager), timestamp, requestor's active window]
    let msg = ClientMessageEvent::new(
        32,
        target,
        atoms._NET_ACTIVE_WINDOW,
        [2, x11rb::CURRENT_TIME, 0, 0, 0],
    );
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        msg,
    )?;
    conn.flush()?;
    Ok(())
}

fn find_keycode(keysyms: &[u32], per: usize, min: u8, wanted: u32) -> Option<u8> {
    if per == 0 {
        return None;
    }
    let row = |pred: &dyn Fn(&[u32]) -> bool| {
        keysyms
            .chunks(per)
            .position(pred)
            .and_then(|i| u8::try_from(i + usize::from(min)).ok())
    };
    // Unshifted column first, then any column (other group / shifted).
    row(&|r| r.first() == Some(&wanted)).or_else(|| row(&|r| r.contains(&wanted)))
}

fn pressed(keymap: &[u8; 32], keycode: u8) -> bool {
    (keymap[usize::from(keycode / 8)] >> (keycode % 8)) & 1 != 0
}

/// Fake key-up for every modifier the user is physically holding.
fn release_held_modifiers(conn: &RustConnection, root: Window) -> Result<()> {
    let keymap = conn.query_keymap()?.reply()?.keys;
    let mods = conn.get_modifier_mapping()?.reply()?;
    let per = usize::from(mods.keycodes_per_modifier());
    if per == 0 {
        return Ok(());
    }
    let mut released = false;
    // Rows: Shift, Lock, Control, Mod1..Mod5. Caps Lock is a toggle; skip it.
    for (row, keys) in mods.keycodes.chunks(per).enumerate() {
        if row == 1 {
            continue;
        }
        for &kc in keys.iter().filter(|&&k| k != 0 && pressed(&keymap, k)) {
            conn.xtest_fake_input(KEY_RELEASE_EVENT, kc, 0, root, 0, 0, 0)?;
            released = true;
        }
    }
    conn.flush()?;
    if released {
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

/// See [`crate::paste_into`].
pub(in crate::linux) fn paste_into(target: ForeignWindow) -> Result<()> {
    let (conn, atoms, root) = session()?;
    let win = u32::try_from(target.0).context("invalid X11 window id")?;
    if conn.get_window_attributes(win)?.reply().is_err() {
        bail!("target window no longer exists");
    }
    // XTEST needs to exist for the key events; check before touching focus.
    conn.xtest_get_version(2, 2)?
        .reply()
        .context("XTEST extension unavailable; cannot send Ctrl+V")?;

    activate(&conn, &atoms, root, win)?;
    if !wait_until(EWMH_WAIT, || is_active(&conn, &atoms, root, win)) {
        tracing::debug!("_NET_ACTIVE_WINDOW did not take effect; trying XSetInputFocus");
        conn.map_window(win)?;
        conn.configure_window(win, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE))?;
        conn.set_input_focus(InputFocus::PARENT, win, x11rb::CURRENT_TIME)?;
        conn.flush()?;
        let ok = wait_until(FOCUS_WAIT, || {
            is_active(&conn, &atoms, root, win) || focus_in_target(&conn, root, win)
        });
        if !ok {
            bail!("could not bring the target window to the foreground");
        }
    }
    // Let the target's input handling pick up the activation.
    std::thread::sleep(Duration::from_millis(15));

    let min = conn.setup().min_keycode;
    let max = conn.setup().max_keycode;
    let map = conn
        .get_keyboard_mapping(min, max - min + 1)?
        .reply()
        .context("reading the keyboard mapping")?;
    let per = usize::from(map.keysyms_per_keycode);
    let ctrl =
        find_keycode(&map.keysyms, per, min, XK_CONTROL_L).unwrap_or(KEYCODE_CTRL_L_FALLBACK);
    let v = find_keycode(&map.keysyms, per, min, XK_V).unwrap_or(KEYCODE_V_FALLBACK);

    release_held_modifiers(&conn, root)?;
    // Terminals paste with Ctrl+Shift+V.
    let shift = wm_class(&conn, &atoms, root, win)
        .as_deref()
        .is_some_and(is_terminal_class)
        .then(|| {
            find_keycode(&map.keysyms, per, min, XK_SHIFT_L).unwrap_or(KEYCODE_SHIFT_L_FALLBACK)
        });
    let mut seq = vec![(KEY_PRESS_EVENT, ctrl)];
    if let Some(shift) = shift {
        seq.push((KEY_PRESS_EVENT, shift));
    }
    seq.push((KEY_PRESS_EVENT, v));
    seq.push((KEY_RELEASE_EVENT, v));
    if let Some(shift) = shift {
        seq.push((KEY_RELEASE_EVENT, shift));
    }
    seq.push((KEY_RELEASE_EVENT, ctrl));
    for (kind, code) in seq {
        conn.xtest_fake_input(kind, code, 0, root, 0, 0, 0)?;
    }
    conn.flush()?;
    // Round trip so the events are processed before we disconnect.
    conn.get_input_focus()?.reply()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keycode_lookup_prefers_unshifted_column() {
        // min keycode 8, 2 keysyms per keycode: [a A] [v V] [V v]
        let ks = [0x61, 0x41, 0x76, 0x56, 0x56, 0x76];
        assert_eq!(find_keycode(&ks, 2, 8, 0x76), Some(9));
        assert_eq!(find_keycode(&ks, 2, 8, 0x56), Some(10));
        assert_eq!(find_keycode(&ks, 2, 8, 0x1234), None);
        assert_eq!(find_keycode(&ks, 0, 8, 0x76), None);
        // keysym only in a shifted column still resolves
        assert_eq!(find_keycode(&[0x61, 0x76], 2, 8, 0x76), Some(8));
    }

    #[test]
    fn keymap_bit_test() {
        let mut km = [0u8; 32];
        km[4] = 1 << 5; // keycode 37
        assert!(pressed(&km, 37));
        assert!(!pressed(&km, 36));
    }

    #[test]
    fn terminal_classes() {
        assert!(is_terminal_class("konsole"));
        assert!(is_terminal_class("Gnome-terminal"));
        assert!(is_terminal_class("Alacritty"));
        assert!(!is_terminal_class("chrome"));
        assert!(!is_terminal_class("kwrite"));
        // Integrated terminals: editor windows stay on Ctrl+V.
        assert!(!is_terminal_class("code"));
    }
}

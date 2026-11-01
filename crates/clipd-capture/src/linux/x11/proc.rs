//! Window -> process name resolution (`_NET_WM_PID` + `/proc`).

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt, Window};

use super::Atoms;

/// How far to walk up the window tree looking for a `_NET_WM_PID`.
const MAX_PARENT_HOPS: usize = 8;

fn cardinal(conn: &impl Connection, win: Window, prop: Atom, ty: AtomEnum) -> Option<u32> {
    conn.get_property(false, win, prop, ty, 0, 1)
        .ok()?
        .reply()
        .ok()?
        .value32()?
        .next()
}

fn pid_of(conn: &impl Connection, atoms: &Atoms, win: Window) -> Option<u32> {
    cardinal(conn, win, atoms._NET_WM_PID, AtomEnum::CARDINAL).filter(|&p| p != 0)
}

/// Pid for `win`: its own `_NET_WM_PID`, its client leader's, then the same
/// for each ancestor. Selection owners are often unmapped helper windows
/// without the property, but their toplevel or leader has it.
pub(super) fn pid_of_window_tree(
    conn: &impl Connection,
    atoms: &Atoms,
    root: Window,
    mut win: Window,
) -> Option<u32> {
    for _ in 0..MAX_PARENT_HOPS {
        if win == x11rb::NONE || win == root {
            return None;
        }
        if let Some(pid) = pid_of(conn, atoms, win) {
            return Some(pid);
        }
        if let Some(leader) = cardinal(conn, win, atoms.WM_CLIENT_LEADER, AtomEnum::WINDOW)
            && let Some(pid) = pid_of(conn, atoms, leader)
        {
            return Some(pid);
        }
        win = conn.query_tree(win).ok()?.reply().ok()?.parent;
    }
    None
}

/// `_NET_ACTIVE_WINDOW` of the root window, if the WM maintains it.
pub(super) fn active_window(conn: &impl Connection, atoms: &Atoms, root: Window) -> Option<Window> {
    cardinal(conn, root, atoms._NET_ACTIVE_WINDOW, AtomEnum::WINDOW).filter(|&w| w != 0)
}

/// Short process name for `pid`: `/proc/<pid>/comm`, or the executable's
/// file name when `comm` was truncated at 15 characters.
pub(super) fn process_name(pid: u32) -> Option<String> {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()?
        .trim()
        .to_string();
    if comm.len() >= 15
        && let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe"))
        && let Some(name) = exe.file_name().and_then(|n| n.to_str())
    {
        let name = name.trim_end_matches(" (deleted)");
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    (!comm.is_empty()).then_some(comm)
}

// Hotkey strings use the format the Rust global-shortcut parser accepts:
// "Ctrl+Shift+Space", "Alt+F2", "Super+V".

const MODS = ["Ctrl", "Alt", "Shift", "Super"] as const;

/** Build an accelerator from a keydown, or null while only modifiers are held. */
export function accelFromEvent(e: KeyboardEvent): string | null {
  const code = e.code;
  if (/^(Control|Alt|Shift|Meta|OS)(Left|Right)?$/.test(code) || code === "") return null;
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");
  let key = code;
  if (/^Key[A-Z]$/.test(code)) key = code.slice(3);
  else if (/^Digit\d$/.test(code)) key = code.slice(5);
  return [...mods, key].join("+");
}

/** A global hotkey needs a modifier unless it's a function key. */
export function validAccel(a: string): string | null {
  const parts = a.split("+");
  const key = parts[parts.length - 1];
  const hasMod = parts.slice(0, -1).some((p) => (MODS as readonly string[]).includes(p));
  if (!hasMod && !/^F\d{1,2}$/.test(key)) return "Add Ctrl, Alt, Shift or Win to the shortcut";
  if (parts.length === 2 && parts[0] === "Shift" && !/^F\d{1,2}$/.test(key)) return "Shift alone would block typing";
  return null;
}

const PRETTY: Record<string, string> = {
  Super: "Win",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Enter: "Enter",
  Escape: "Esc",
};

export function prettyKeys(a: string): string[] {
  return a.split("+").map((k) => PRETTY[k] ?? k);
}

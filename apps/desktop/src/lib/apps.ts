// Friendly names for source apps. Windows reports executables ("chrome.exe"),
// Linux reports process names ("firefox", "code"). Unknown names are shown
// with the extension stripped and the first letter capitalized.

const NAMES: Record<string, string> = {
  chrome: "Chrome",
  msedge: "Edge",
  firefox: "Firefox",
  brave: "Brave",
  arc: "Arc",
  opera: "Opera",
  vivaldi: "Vivaldi",
  code: "VS Code",
  "code-oss": "VS Code",
  codium: "VSCodium",
  cursor: "Cursor",
  zed: "Zed",
  devenv: "Visual Studio",
  idea64: "IntelliJ IDEA",
  idea: "IntelliJ IDEA",
  pycharm64: "PyCharm",
  rustrover64: "RustRover",
  studio64: "Android Studio",
  windowsterminal: "Terminal",
  wt: "Terminal",
  "gnome-terminal-server": "Terminal",
  konsole: "Konsole",
  alacritty: "Alacritty",
  wezterm: "WezTerm",
  "wezterm-gui": "WezTerm",
  kitty: "kitty",
  powershell: "PowerShell",
  pwsh: "PowerShell",
  cmd: "Command Prompt",
  explorer: "File Explorer",
  nautilus: "Files",
  dolphin: "Dolphin",
  notepad: "Notepad",
  "notepad++": "Notepad++",
  winword: "Word",
  excel: "Excel",
  powerpnt: "PowerPoint",
  outlook: "Outlook",
  olk: "Outlook",
  "ms-teams": "Teams",
  teams: "Teams",
  slack: "Slack",
  discord: "Discord",
  telegram: "Telegram",
  whatsapp: "WhatsApp",
  messages: "Messages",
  figma: "Figma",
  notion: "Notion",
  obsidian: "Obsidian",
  linear: "Linear",
  spotify: "Spotify",
  snippingtool: "Snipping Tool",
  screenclippinghost: "Snipping Tool",
  sharex: "ShareX",
  flameshot: "Flameshot",
  "gnome-screenshot": "Screenshot",
  thunderbird: "Thunderbird",
  libreoffice: "LibreOffice",
  soffice: "LibreOffice",
  gedit: "Text Editor",
  "gnome-text-editor": "Text Editor",
  kate: "Kate",
};

function base(app: string): string {
  const s = app.split(/[\\/]/).pop() ?? app;
  return s.replace(/\.(exe|app|appimage|bin)$/i, "");
}

export function appName(app: string | null | undefined): string {
  if (!app) return "Unknown app";
  const b = base(app);
  const known = NAMES[b.toLowerCase()];
  if (known) return known;
  return b.charAt(0).toUpperCase() + b.slice(1);
}

/** Stable hue per app for the letter avatar. */
export function appHue(app: string | null | undefined): number {
  const s = base(app ?? "?").toLowerCase();
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) >>> 0;
  return h % 360;
}

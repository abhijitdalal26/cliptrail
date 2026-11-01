// Friendly names for source apps. Windows reports executables ("chrome.exe"),
// Linux reports process names ("firefox", "code"), Android reports package
// names ("com.android.chrome"). Unknown names are shown with the extension
// stripped and the first letter capitalized; unknown Android packages by
// their most telling segment.

const ANDROID: Record<string, string> = {
  "com.android.chrome": "Chrome",
  "org.mozilla.firefox": "Firefox",
  "com.sec.android.app.sbrowser": "Samsung Internet",
  "com.microsoft.emmx": "Edge",
  "com.brave.browser": "Brave",
  "com.opera.browser": "Opera",
  "com.duckduckgo.mobile.android": "DuckDuckGo",
  "com.google.android.gm": "Gmail",
  "com.samsung.android.email.provider": "Samsung Email",
  "com.microsoft.office.outlook": "Outlook",
  "com.whatsapp": "WhatsApp",
  "com.whatsapp.w4b": "WhatsApp Business",
  "org.telegram.messenger": "Telegram",
  "org.thoughtcrime.securesms": "Signal",
  "com.google.android.apps.messaging": "Messages",
  "com.samsung.android.messaging": "Samsung Messages",
  "com.slack": "Slack",
  "com.discord": "Discord",
  "com.microsoft.teams": "Teams",
  "com.instagram.android": "Instagram",
  "com.twitter.android": "X",
  "com.reddit.frontpage": "Reddit",
  "com.linkedin.android": "LinkedIn",
  "com.google.android.youtube": "YouTube",
  "com.google.android.apps.maps": "Maps",
  "com.google.android.keep": "Keep",
  "com.google.android.apps.docs": "Drive",
  "com.google.android.apps.photos": "Photos",
  "com.sec.android.gallery3d": "Gallery",
  "com.samsung.android.app.notes": "Samsung Notes",
  "com.samsung.android.app.smartcapture": "Screenshot",
  "com.sec.android.app.myfiles": "My Files",
  "com.android.settings": "Settings",
  "com.notion.id": "Notion",
  "md.obsidian": "Obsidian",
  "com.termux": "Termux",
  "com.github.android": "GitHub",
  "com.spotify.music": "Spotify",
  "dev.cliptrail.app": "ClipTrail",
};
/** Package segments that say nothing about the app. */
const GENERIC = new Set(["com", "org", "net", "dev", "io", "app", "android", "mobile", "google", "apps", "free", "pro", "lite", "client"]);

function androidName(pkg: string): string | null {
  if (!/^[a-z][\w]*(\.[\w]+)+$/i.test(pkg) || /\.(exe|app|appimage|bin)$/i.test(pkg)) return null;
  const known = ANDROID[pkg.toLowerCase()];
  if (known) return known;
  const parts = pkg.split(".").filter((p) => !GENERIC.has(p.toLowerCase()));
  const pick = parts[parts.length - 1] ?? pkg.split(".").pop() ?? pkg;
  return NAMES[pick.toLowerCase()] ?? pick.charAt(0).toUpperCase() + pick.slice(1);
}

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
  const android = androidName(app);
  if (android) return android;
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

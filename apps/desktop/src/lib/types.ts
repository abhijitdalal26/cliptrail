// TypeScript mirror of crates/clipd-core/src/model.rs (the JSON contract).
// Keep in sync with that file.

export type Millis = number;

export type Kind =
  | "link"
  | "code"
  | "text"
  | "contact"
  | "color"
  | "file"
  | "image"
  | "sensitive";

export const ALL_KINDS: Kind[] = [
  "link",
  "code",
  "text",
  "contact",
  "color",
  "file",
  "image",
  "sensitive",
];

export interface ClipSummary {
  id: string;
  kind: Kind;
  subkind: string | null;
  /** First ~400 chars (masked clips: "••••••"). Empty for images. */
  preview: string;
  truncated: boolean;
  char_count: number;
  line_count: number;
  files: string[];
  /** Absolute path of the thumbnail PNG (images only). */
  thumb_path: string | null;
  width: number | null;
  height: number | null;
  source_app: string | null;
  device_id: string;
  created_at: Millis;
  last_seen_at: Millis;
  copy_count: number;
  pinned: boolean;
  masked: boolean;
  expires_at: Millis | null;
}

export interface Clip extends ClipSummary {
  text: string | null;
  html: string | null;
  image_path: string | null;
}

export interface ListQuery {
  kind?: Kind | null;
  search?: string | null;
  pinned_only?: boolean;
  source_app?: string | null;
  from?: Millis | null;
  to?: Millis | null;
  before?: Millis | null;
  /** Tie-breaker for `before`: pass the last row's id too (rows sort by
   *  last_seen_at DESC, id DESC). */
  before_id?: string | null;
  limit?: number | null;
}

export interface Stats {
  total: number;
  by_kind: [Kind, number][];
  pinned: number;
  top_apps: [string, number][];
}

export interface DayCount {
  /** "YYYY-MM-DD", local time. */
  day: string;
  count: number;
}

export type IngestOutcome =
  | { outcome: "stored"; id: string; kind: Kind }
  | { outcome: "bumped"; id: string; kind: Kind }
  | { outcome: "skipped"; reason: string };

// ---- desktop app (src-tauri) types ----

export type ThemePref = "system" | "light" | "dark";

export interface Settings {
  picker_shortcut: string;
  dashboard_shortcut: string | null;
  theme: ThemePref;
  retention_days: number | null;
  sensitive_ttl_minutes: number;
  excluded_apps: string[];
  /** Strip tracking parameters from copied links before they are stored. */
  clean_links: boolean;
  /** Run the LAN sync hub (changed through syncApi.setEnabled). */
  sync_enabled?: boolean;
  /** Sync hub port (changed through syncApi.setPort). */
  sync_port?: number;
}

export interface CaptureState {
  paused: boolean;
  /** Unix ms when capture resumes automatically; null = until resumed. */
  until: Millis | null;
}

export interface SettingsView {
  settings: Settings;
  default_excluded_apps: string[];
  autostart: boolean;
  data_dir: string;
  version: string;
  capture: CaptureState;
  platform: string;
}

// ---- type-aware actions (mirror of clipd_core::transform::Transform) ----

export type Transform =
  | "plain-text"
  | "trim"
  | "upper-case"
  | "lower-case"
  | "title-case"
  | "single-line"
  | "clean-url"
  | "markdown-link"
  | "url-decode"
  | "color-hex"
  | "color-rgb"
  | "color-hsl"
  | "json-pretty"
  | "json-minify"
  | "markdown-code-block"
  | "dedent"
  | "phone-digits"
  | "mailto-link";

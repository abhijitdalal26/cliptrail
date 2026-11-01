import {
  Code2,
  FileText,
  Folder,
  Image as ImageIcon,
  Link2,
  Palette,
  ShieldAlert,
  UserRound,
  type LucideIcon,
} from "lucide-react";
import type { Kind } from "./types";

export interface KindMeta {
  label: string;
  plural: string;
  icon: LucideIcon;
}

export const KIND_META: Record<Kind, KindMeta> = {
  link: { label: "Link", plural: "Links", icon: Link2 },
  code: { label: "Code", plural: "Code", icon: Code2 },
  text: { label: "Text", plural: "Text", icon: FileText },
  contact: { label: "Contact", plural: "Contacts", icon: UserRound },
  color: { label: "Color", plural: "Colors", icon: Palette },
  file: { label: "File", plural: "Files", icon: Folder },
  image: { label: "Image", plural: "Images", icon: ImageIcon },
  sensitive: { label: "Sensitive", plural: "Sensitive", icon: ShieldAlert },
};

/** Pretty names for code language subkinds. */
const LANG_NAMES: Record<string, string> = {
  rust: "Rust",
  python: "Python",
  javascript: "JavaScript",
  typescript: "TypeScript",
  json: "JSON",
  sql: "SQL",
  shell: "Shell",
  bash: "Bash",
  powershell: "PowerShell",
  go: "Go",
  java: "Java",
  kotlin: "Kotlin",
  swift: "Swift",
  c: "C",
  cpp: "C++",
  csharp: "C#",
  html: "HTML",
  xml: "XML",
  css: "CSS",
  yaml: "YAML",
  toml: "TOML",
  markdown: "Markdown",
  ruby: "Ruby",
  php: "PHP",
  diff: "Diff",
};

export function langName(sub: string | null | undefined): string | null {
  if (!sub) return null;
  return LANG_NAMES[sub.toLowerCase()] ?? sub.charAt(0).toUpperCase() + sub.slice(1);
}

const SENSITIVE_NAMES: Record<string, string> = {
  otp: "One-time code",
  "api-key": "API key",
  apikey: "API key",
  token: "Token",
  card: "Card number",
  "private-key": "Private key",
  password: "Password",
};

export function subkindLabel(kind: Kind, sub: string | null): string | null {
  if (!sub) return null;
  if (kind === "code") return langName(sub);
  if (kind === "sensitive") return SENSITIVE_NAMES[sub] ?? sub;
  if (kind === "contact") return sub === "email" ? "Email" : sub === "phone" ? "Phone" : sub;
  if (kind === "color") return sub.toUpperCase();
  if (kind === "image") return sub === "screenshot" ? "Screenshot" : sub;
  if (kind === "file") return null;
  return sub;
}

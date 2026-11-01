// Display metadata for type-aware actions (see crates/clipd-core/src/transform.rs).

import {
  Braces,
  CaseLower,
  CaseSensitive,
  CaseUpper,
  FileCode2,
  IndentDecrease,
  Link2,
  Mail,
  Minimize2,
  Percent,
  Phone,
  Pipette,
  Scissors,
  Sparkles,
  WrapText,
  ClipboardType,
  type LucideIcon,
} from "lucide-react";
import type { Transform } from "./types";

export interface ActionMeta {
  /** Button label. */
  label: string;
  icon: LucideIcon;
  /** Toast after copying the result. */
  done: string;
}

export const ACTION_META: Record<Transform, ActionMeta> = {
  "plain-text": { label: "Plain text", icon: ClipboardType, done: "Copied as plain text" },
  trim: { label: "Trim whitespace", icon: Scissors, done: "Copied trimmed text" },
  "upper-case": { label: "UPPERCASE", icon: CaseUpper, done: "Copied in uppercase" },
  "lower-case": { label: "lowercase", icon: CaseLower, done: "Copied in lowercase" },
  "title-case": { label: "Title Case", icon: CaseSensitive, done: "Copied in title case" },
  "single-line": { label: "Join into one line", icon: WrapText, done: "Copied as one line" },
  "clean-url": { label: "Clean link", icon: Sparkles, done: "Copied clean link" },
  "markdown-link": { label: "As Markdown link", icon: Link2, done: "Copied Markdown link" },
  "url-decode": { label: "Decode URL", icon: Percent, done: "Copied decoded URL" },
  "color-hex": { label: "Copy as HEX", icon: Pipette, done: "Copied HEX" },
  "color-rgb": { label: "Copy as RGB", icon: Pipette, done: "Copied RGB" },
  "color-hsl": { label: "Copy as HSL", icon: Pipette, done: "Copied HSL" },
  "json-pretty": { label: "Prettify JSON", icon: Braces, done: "Copied prettified JSON" },
  "json-minify": { label: "Minify JSON", icon: Minimize2, done: "Copied minified JSON" },
  "markdown-code-block": { label: "As Markdown code block", icon: FileCode2, done: "Copied Markdown code block" },
  dedent: { label: "Dedent", icon: IndentDecrease, done: "Copied dedented text" },
  "phone-digits": { label: "Digits only", icon: Phone, done: "Copied phone digits" },
  "mailto-link": { label: "As mailto link", icon: Mail, done: "Copied mailto link" },
};

/** How many actions are shown inline before the rest move into "More". */
export const PRIMARY_ACTIONS = 5;

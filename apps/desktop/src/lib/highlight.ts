// Lazy-loaded syntax highlighting (highlight.js core + a curated language
// set, ~40 KB gz, fetched the first time a code clip is shown).

import { useEffect, useState } from "react";

type Hljs = typeof import("highlight.js/lib/core").default;

let loader: Promise<Hljs> | null = null;
let ready: Hljs | null = null;

const ALIASES: Record<string, string> = {
  shell: "bash",
  sh: "bash",
  zsh: "bash",
  ps1: "powershell",
  js: "javascript",
  jsx: "javascript",
  ts: "typescript",
  tsx: "typescript",
  py: "python",
  rs: "rust",
  html: "xml",
  svg: "xml",
  toml: "ini",
  yml: "yaml",
  "c++": "cpp",
  "c#": "csharp",
  cs: "csharp",
  md: "markdown",
  kt: "kotlin",
};

function load(): Promise<Hljs> {
  loader ??= (async () => {
    const [core, ...langs] = await Promise.all([
      import("highlight.js/lib/core"),
      import("highlight.js/lib/languages/rust"),
      import("highlight.js/lib/languages/python"),
      import("highlight.js/lib/languages/javascript"),
      import("highlight.js/lib/languages/typescript"),
      import("highlight.js/lib/languages/json"),
      import("highlight.js/lib/languages/sql"),
      import("highlight.js/lib/languages/bash"),
      import("highlight.js/lib/languages/powershell"),
      import("highlight.js/lib/languages/go"),
      import("highlight.js/lib/languages/java"),
      import("highlight.js/lib/languages/kotlin"),
      import("highlight.js/lib/languages/swift"),
      import("highlight.js/lib/languages/c"),
      import("highlight.js/lib/languages/cpp"),
      import("highlight.js/lib/languages/csharp"),
      import("highlight.js/lib/languages/xml"),
      import("highlight.js/lib/languages/css"),
      import("highlight.js/lib/languages/yaml"),
      import("highlight.js/lib/languages/ini"),
      import("highlight.js/lib/languages/markdown"),
      import("highlight.js/lib/languages/ruby"),
      import("highlight.js/lib/languages/php"),
      import("highlight.js/lib/languages/diff"),
    ]);
    const names = [
      "rust", "python", "javascript", "typescript", "json", "sql", "bash", "powershell", "go", "java",
      "kotlin", "swift", "c", "cpp", "csharp", "xml", "css", "yaml", "ini", "markdown", "ruby", "php", "diff",
    ];
    const hljs = core.default;
    names.forEach((n, i) => hljs.registerLanguage(n, langs[i].default));
    ready = hljs;
    return hljs;
  })();
  return loader;
}

const cache = new Map<string, string>();

function render(hljs: Hljs, code: string, lang: string | null | undefined): string {
  const key = `${lang ?? ""}\u0000${code}`;
  const hit = cache.get(key);
  if (hit != null) return hit;
  const l = lang ? (ALIASES[lang.toLowerCase()] ?? lang.toLowerCase()) : null;
  let html: string;
  try {
    html =
      l && hljs.getLanguage(l)
        ? hljs.highlight(code, { language: l, ignoreIllegals: true }).value
        : hljs.highlightAuto(code).value;
  } catch {
    html = escapeHtml(code);
  }
  if (cache.size > 400) cache.clear();
  cache.set(key, html);
  return html;
}

export function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
}

/** Highlighted HTML for `code` (escaped plain text until the highlighter loads). */
export function useHighlighted(code: string, lang: string | null | undefined): string {
  const [, force] = useState(0);
  useEffect(() => {
    if (!ready) {
      let alive = true;
      void load().then(() => alive && force((n) => n + 1));
      return () => {
        alive = false;
      };
    }
  }, []);
  return ready ? render(ready, code, lang) : escapeHtml(code);
}

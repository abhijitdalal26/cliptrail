# Feature candidates — final research pass

Last sweep before building. Two sections:

1. **Products we missed** — what they do, what they lack, what users ask them for.
2. **Candidate features** — every ask found in the wild, with evidence. Each is marked **[A]** recommended now, **[B]** good later, **[C]** probably skip. Review and strike/change.

## 1. Products we missed

### Sync / cross-device tools

| Product | What it is | What it lacks / what users beg for |
| --- | --- | --- |
| **Deskflow / Input Leap / Lan Mouse / Synergy** | Software KVMs — share keyboard+mouse+clipboard across machines | Clipboard sharing only works while both machines on; no history; **Wayland clipboard "rarely works"** (open complaints); no mobile. Validates "LAN sync is table stakes, history+mobile is the gap." |
| **ClipboardFusion** (Binary Fortress) | Windows/Mac/iOS manager; text scrubbing, C# macro triggers, 30 pinned hotkeys | Sync across devices is **paid-only**, no history depth, no Android, closed source. Their triggers/macros prove demand for transforms+automation on copy. |
| **PDB (Personal Device Bridge)** | Windows↔Android; push model: Ctrl+Shift+B sends clipboard to a picked device; synced snippets | No history manager. Their whole design is **"send on purpose, never auto-sync everything"** — users explicitly prefer it (see GSConnect evidence below). |
| **pastehere.app / Remote Clipboard (rclip)** | Web "rooms": open link/scan QR on two devices → shared clipboard, no signup; rclip has a CLI | Latest-clip only, no history, no local app. But the **QR-room = zero-install pairing** idea is worth stealing (see candidate: quick-share link). |
| **SnipTray** | Mac/iOS/iPad manager, iCloud sync, team pinboards | Apple-only, closed, 25-item free cap. First mover on **shared clipboards between people** — our "space" model could do this on any OS. |
| **1Clipboard** | Electron + Google Drive sync, cross-platform | Dead since 2016 — Google-Drive-as-sync was fragile; lesson: own the sync protocol, don't bolt it to a consumer API. |
| **SyncClipboard users** | — | Most-upvoted ask: **use S3/OSS/COS object storage as the sync server** (8 reactions) — self-hosters want sync without running a live server. |

### Single-device managers (history/UX benchmarks)

| Product | What users ask/beg for |
| --- | --- |
| **Maccy** (macOS, free OSS) | #1 open issue: **unlimited history** (60 reactions) — cap is 200; paste/delete **multiple items** (34); image support; sync; iOS app; memory-growth complaints (41) — our Rust daemon dodges this |
| **Ditto** (Windows) | #1 ask: **export/import settings** (12); **encrypt the database** (8); exclude-websites rule (5); daily backup (5); "send as raw keystrokes" for apps that block paste (7); mobile app |
| **CopyQ** (Linux/cross) | #1: **Wayland support** (29, closed-unresolved — still broken); #2: **unlimited history** (27); sequential paste requested (issue #3348) |
| **ClipClip** (Windows, closed) | Feature-rich benchmark: clip *editors* (RTF/HTML/image annotate), OCR, translate, templates, Google Drive folder sync, 1000-clip cap. No sync, no mobile |
| **ClipBook** (macOS) | Keyboard-first. Shipped **copy-sequence**: copies within 30s group into one sequence you can paste in order without opening history — sequential paste, done elegantly |
| **Beetroot** (Windows, closed releases) | Sequential paste requested and shortlisted by the dev — demand is current (Apr 2026) |
| **Rememory** (Windows, MIT) | Unlimited history; clean UI — minor benchmark |
| **Raycast / Alfred clipboard** (macOS launchers) | Raycast keeps **every original format** per clip and offers "Paste as…" (PNG/RTF/HTML/plain); groups multi-copies into one entry; rename clips; QR-code text extraction; OCR from image. Alfred users ask for snippets-in-history |
| **PTHPasteboard** (old Mac) | Dead; users still nostalgic — "sync + power features + minimal UI" was loved in 2015 |

### Android / mobile

| Product | Note |
| --- | --- |
| **Clipboard Manager (devdnua, Doggo Apps)** | Top Play results: history + pin + search + cloud backup; **"smart actions"** on a clip (translate, Google-search, open-in-app) — a per-clip action menu is a proven mobile pattern |
| **Native Clipboard, EasyCopy** | EasyCopy shows an app-picker on copy — "quick paste to another app" idea |
| **CleverKeys** | Android keyboard that **strips tracking params from copied URLs** — validates our keyboard-capture + adds the link-cleaning pattern |
| **Lift** (iOS) | Cleans tracking params; **screenshot→OCR scan + pattern detection**: pulls confirmation codes, tracking numbers, phone numbers with tap-to-act buttons — on-device, private |
| **Maclip** (macOS) | Auto-cleans tracking params from copied links; per-link "copy clean URL" — link hygiene is now a category |

### Automation / adjacent

| Product | Note |
| --- | --- |
| **SaneClip** (macOS) | **Webhooks** on copy/paste/delete (HMAC-signed, retries), URL schemes (`saneclip://search`, `/paste`), Shortcuts/Alfred/Raycast integration |
| **Clipboard2Webhook** (Chrome ext), **Clipboard Genie** (Win) | Send clips to n8n/Zapier/Make/IFTTT — web-research/automation users pay for this |
| **Codeonix** | Automation tool with a **clipboard-copy trigger** among 31 triggers |
| **PureLink** (OSS daemon) | One-job tool: strips trackers, unshortens links, Dropbox→direct-download, YT shorts→watch URL |

## 2. Candidate features

Already planned (validated again): E2EE sync, unlimited history, Wayland, OCR, semantic search, paste-as-plain-text, snippets, password-manager skip, self-host, paste stack, Ditto/CopyQ import.

**New asks found in the wild:**

### A — recommended into the plan

- [ ] **Push-to-device (selective send)** — send *one* clip to a picked device on purpose, in addition to full history sync. Evidence: GSConnect #86 (user uncomfortable auto-sharing everything → they built clipboard push/pull); PDB's entire product is push-only; HN commenters prefer it. Also fixes the "I don't want password-adjacent stuff on my phone" fear. *Effort: small — reuse event pipeline + a "send to…" picker entry.*
- [ ] **Never-sync / local-only flag** — a "keep on this device" toggle per clip/section/app. Companion to push mode. *Effort: small — one flag checked in sync emit.*
- [ ] **Link hygiene: strip tracking params on copy** — utm_*, fbclid, gclid, si… Evidence: Maclip, PureLink, Lift, CleverKeys all exist *just* for this; ClipboardFusion users do it via scripts. Ours: optional rule that rewrites the clip at normalize time. *Effort: small — param list + URL parse.*
- [ ] **Clipboard transforms/actions** — right-click/hotkey on a clip: paste-as-X (plain, RTF, HTML, PNG), case conversion, Excel-paste→Markdown/JSON/CSV (three dedicated web tools exist = demand), regex replace (ClipboardFusion's #1 power feature). *Effort: medium — start with paste-as + case + TSV→MD.*
- [ ] **Keep original formats per clip** — Raycast stores every format and offers "Paste as…"; Ditto/CopyQ users hit HTML-vs-text issues. Store `text/plain` + `text/html` + `image` variants, pick at paste. *Effort: medium — affects normalize + paste-back.*
- [ ] **Config/settings export + sync** — Ditto's #1 open issue; Maccy asked too. Settings are just another event in our sync log; also one-click export file. *Effort: trivial.*
- [ ] **Auto-backup / export on schedule** — Ditto ask (5). `clipd-cli export --daily` via cron/launchd or built-in timer. *Effort: trivial.*
- [ ] **Sequential paste (upgrade P1 → confirmed demand)** — CopyQ #3348, Beetroot shortlisted, BTT forum, ClipBook shipped "copy sequence" (copies <30s apart = one sequence, paste in order). Adopt ClipBook's sequence model. *Effort: medium.*
- [ ] **QR quick-share to non-users** — "send this clip to a friend/phone without installing": relay hosts a short-lived encrypted room/page (pastehere/rclip pattern). Doubles as onboarding + viral loop. *Effort: medium — relay page + ephemeral space.*
- [ ] **Pattern-detection quick actions** — Lift's winning UX: detect confirmation code/tracking number/phone/email/OTP in a clip → show action buttons (copy code, track parcel, call). Fits our classifier + mobile UI. *Effort: small-medium — extends subkind patterns.*
- [ ] **Exclude by source site/app (rules on metadata, not just content)** — Ditto ask "exclude websites" — our rules already key on source_app; add browser-domain matching. *Effort: small.*
- [ ] **S3/object-storage as self-host backend** — SyncClipboard's most-upvoted request. Optional relay adapter: `clipd-relay-s3` polling a bucket — sync without running a live server. *Effort: medium — defer to post-MVP but keep the envelope format storage-agnostic (it already is).*

### B — good later

- [ ] **Per-clip action menu on Android** — translate / search / open-in-app / share (devdnua pattern) — natural once mobile shell exists.
- [ ] **Rename clips + grouped multi-copy entries** — Raycast niceties: rename for searchability, multi-file copies grouped as one entry.
- [ ] **Compact pinned view + multi-select paste/delete** — Maccy asks (11, 34): pinned lane shouldn't eat the UI; operate on N clips at once.
- [ ] **"Send as keystrokes" paste mode** — Ditto ask: for remote-desktop/fields that block paste. Type the text via synthetic key events.
- [ ] **Image clip editor/annotate** — ClipClip-level polish; later.
- [ ] **Webhooks/CLI hooks on clip events** — SaneClip/Clipboard2Webhook/Codeonix demand: `on_clip → POST` or run a user script. We already have clipd-cli; add an event hook config. *Small, but post-MVP.*
- [ ] **Import from Maccy** (asked on UniClipboard) — join our Ditto/CopyQ importer list.
- [ ] **Intercept Win+V / ⌘V muscle memory** — UniClipboard ask: optionally take over the OS history hotkey.
- [ ] **Time-based auto-clear retention** (Maccy ask) — `expires_at` policies: "keep sensitive 1h, keep rest forever".
- [ ] **Team shared space** — SnipTray is first to ship shared pinboards (Apple-only). Our space = multi-device already; a shared space between 2+ *people* is a post-MVP strategic play (support teams, pair programming). Defer deliberately.

### C — probably skip

- [ ] **Clip editors (RTF/HTML WYSIWYG)** — ClipClip territory; heavy, low frequency vs. cost.
- [ ] **Unshorten links / Dropbox→direct-link conversion** — PureLink does it; niche; fold into link-hygiene as stretch rules if cheap.
- [ ] **OS notification on every copy** — Maccy users complained (16); default = silent capture.
- [ ] **Cloud-drive folder sync** (ClipClip GDrive, 1Clipboard) — fragile and redundant with our protocol.
- [ ] **Translate inline** — ClipClip/devdnua have it; easy later via action menu, not core.

## 3. The meta-finding

Three asks dominate every tracker and forum: **unlimited reliable history** (Maccy 60, CopyQ 27), **sync that actually works cross-OS** (Ditto begged since 2020, Wayland still broken in 2026), and **control over what syncs** (GSConnect/PDB users explicitly reject share-everything).

The pattern nobody has combined: *unlimited history + E2EE sync + selective push + transforms at capture time + a zero-install share path*. That's ClipTrail's pitch. Nothing in this pass invalidates the 3-day plan — it adds the A-list above as cheap inserts (push-to-device, local-only flag, link cleaning, transforms, settings sync, QR quick-share) that each reuse pipeline pieces we already designed.

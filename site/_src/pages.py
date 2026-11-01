"""Bodies of the non-docs pages."""

def shot(name, alt, cls="", eager=False, w=1920, h=1200):
    load = 'loading="eager" fetchpriority="high"' if eager else 'loading="lazy"'
    return (f'<img class="only-light {cls}" src="{{{{R}}}}assets/screens/{name}-light.webp" width="{w}" height="{h}" alt="{alt}" {load} decoding="async">'
            f'<img class="only-dark {cls}" src="{{{{R}}}}assets/screens/{name}-dark.webp" width="{w}" height="{h}" alt="{alt}" loading="lazy" decoding="async">')

HEAT = "".join(f'<i class="l{lvl}"></i>' if lvl else "<i></i>" for lvl in [
    0,0,0,0,0,0,0, 0,0,0,0,0,0,0, 0,0,1,0,0,0,0, 0,1,0,0,1,0,0, 0,0,0,0,0,0,0, 1,0,1,0,0,1,0,
    1,2,1,0,1,0,0, 0,1,2,1,2,1,0, 2,1,0,2,1,0,0, 0,2,3,1,2,2,1, 1,1,0,2,3,1,0, 2,3,2,1,2,0,1,
    1,2,3,2,4,1,0, 3,2,1,3,2,2,1, 2,4,3,2,3,1,1, 3,2,4,3,0,0,0])

CHECK = '<span class="chk">{{i:check}}</span>'
TODO = '<span class="chk todo">{{i:dash}}</span>'

INDEX = r"""
<section class="hero">
  <div class="wrap">
    <div class="hero-copy">
      <a class="badge" href="{{R}}changelog.html"><span class="dot"></span>Pre-release · v0.1.0 alpha is coming <span class="arrow">→</span></a>
      <h1 class="h1">Your clipboard,<br><span class="grad">everywhere, forever.</span></h1>
      <p class="lede">{{B}} remembers everything you copy, knows what each thing is, and puts it one keystroke away. Pair your devices and your history follows you, end-to-end encrypted. Open source, no account, no tracking.</p>
      <div class="hero-cta">
        <a class="btn btn-primary btn-lg" href="{{R}}download.html" data-os-cta>{{i:download}}<span data-os-cta-label>Download {{B}}</span></a>
        <a class="btn btn-lg" href="{{R}}docs/index.html">{{i:book}}Read the docs</a>
        <a class="btn btn-lg btn-ghost" href="{{REPO}}">{{i:github}}Star on GitHub</a>
      </div>
      <p class="hero-note"><span data-os-note>Windows · Linux · macOS (experimental)</span> · Free and open source</p>
      <div class="platform-row" aria-label="Platform support">
        <span class="pill"><span class="status-dot ok"></span><b>Windows</b></span>
        <span class="pill"><span class="status-dot beta"></span><b>Linux</b> beta</span>
        <span class="pill"><span class="status-dot soon"></span><b>Android</b> coming in v0.1</span>
        <span class="pill"><span class="status-dot exp"></span><b>macOS</b> experimental</span>
        <span class="pill"><span class="status-dot"></span><b>iOS</b> later</span>
      </div>
    </div>
    <div class="showcase">
      <div class="window window-main">""" + shot("dashboard", "The {{B}} dashboard: a sidebar of kinds and apps, a calendar heatmap, a day-by-day timeline of clips and a preview of the selected code clip.", eager=True) + r"""</div>
      <div class="window-float">""" + shot("picker", "The quick picker: a search box, kind tabs and a list of recent clips with Ctrl+1 to Ctrl+9 quick-paste hints.", w=1200, h=891) + r"""</div>
      <span class="callout-tag t1">{{i:keyboard}}<kbd>Ctrl</kbd><kbd>Shift</kbd><kbd>Space</kbd> from any app</span>
      <span class="callout-tag t2">{{i:lock}}Synced end-to-end encrypted</span>
    </div>
  </div>
</section>

<section class="section-tight">
  <div class="wrap">
    <div class="steps">
      <div class="step"><span class="num">1</span><h2>Copy like you always do</h2><p>{{B}} saves every copy in the background and works out what it is: a link, code, a colour, an image.</p></div>
      <div class="step"><span class="num">2</span><h2>Summon the picker</h2><p>Press the hotkey in any app and type a few letters. Search runs over years of history as you type.</p><span class="keys"><kbd>Ctrl</kbd><kbd>Shift</kbd><kbd>Space</kbd></span></div>
      <div class="step"><span class="num">3</span><h2>Press Enter</h2><p>The clip goes straight into the app you were in. <kbd>Shift</kbd> <kbd>Enter</kbd> pastes it as plain text.</p><span class="keys"><kbd>Enter</kbd></span></div>
    </div>
  </div>
</section>

<section class="section section-alt" id="features">
  <div class="wrap">
    <div class="section-head">
      <span class="eyebrow">Features</span>
      <h2 class="h2">Every copy, kept, sorted and one keystroke away.</h2>
      <p class="lede">Built-in clipboards forget after a few dozen items or a restart. Sync tools only move your last copy. {{B}} keeps the whole trail and makes it easy to find.</p>
    </div>
    <div class="bento">
      <div class="tile span-3">
        <span class="ico">{{i:keyboard}}</span>
        <h3>Instant picker, paste where you were</h3>
        <p>A hotkey opens a search box over whatever you're doing. Enter pastes into the previous app; <kbd>Ctrl</kbd> <kbd>1</kbd>–<kbd>9</kbd> quick-pastes the top nine; <kbd>Tab</kbd> flips through kinds.</p>
        <a class="more" href="{{R}}docs/picker.html">How the picker works →</a>
        <div class="tile-media inset">""" + shot("picker", "Quick picker listing recent clips.", cls="shot", w=1200, h=891) + r"""</div>
      </div>
      <div class="tile span-3">
        <span class="ico">{{i:sparkles}}</span>
        <h3>Smart actions for every kind</h3>
        <p>Clean tracking junk from links, copy a colour as RGB or HSL, prettify or minify JSON, wrap code in a Markdown block, dedent, join lines, change case. Each action shows a live preview.</p>
        <a class="more" href="{{R}}docs/smart-actions.html">All smart actions →</a>
        <div class="tile-media inset">""" + shot("actions", "Picker action list for a colour clip offering Copy as RGB and Copy as HSL with previews.", cls="shot", w=1200, h=891) + r"""</div>
      </div>

      <div class="tile span-2">
        <span class="ico">{{i:history}}</span>
        <h3>Unlimited history</h3>
        <p>Text, links, code, images and files, kept until you delete them, or for as long as you choose. Nothing lost after a restart.</p>
        <ul class="mini-list">
          <li>{{i:check}}Keep forever, 1 year, 90 or 30 days</li>
          <li>{{i:check}}Pinned clips are never cleaned up</li>
          <li>{{i:check}}Repeat copies bump, never duplicate</li>
        </ul>
      </div>
      <div class="tile span-4">
        <span class="ico">{{i:tag}}</span>
        <h3>Knows what you copied</h3>
        <p>Every clip is recognised on your device the moment you copy it, with no tagging and no cloud. Code gets its language (Rust, Python, TypeScript, SQL, shell and 15 more), links get their site, secrets get hidden.</p>
        <div class="kind-cloud">
          <span class="kind k-link">{{i:link}}Link</span>
          <span class="kind k-code">{{i:code}}Code</span>
          <span class="kind k-text">{{i:text}}Text</span>
          <span class="kind k-contact">{{i:contact}}Contact</span>
          <span class="kind k-color">{{i:color}}Color</span>
          <span class="kind k-file">{{i:file}}File</span>
          <span class="kind k-image">{{i:image}}Image</span>
          <span class="kind k-secret">{{i:secret}}Sensitive</span>
        </div>
        <a class="more" href="{{R}}docs/kinds.html">How classification works →</a>
      </div>

      <div class="tile span-4">
        <span class="ico">{{i:calendar}}</span>
        <h3>Travel back in time</h3>
        <p>A timeline grouped by day and a 16-week calendar heatmap. Click a day to jump straight to "that thing I copied last Tuesday", or narrow to today, 7 or 30 days, a kind or an app.</p>
        <div class="heat" role="img" aria-label="Calendar heatmap of clips per day over 16 weeks">""" + HEAT + r"""</div>
      </div>
      <div class="tile span-2">
        <span class="ico">{{i:tray}}</span>
        <h3>Lives in your tray</h3>
        <p>Recent clips, pause capture, start at login, all one click away.</p>
        <div class="tray-menu" aria-hidden="true">
          <div class="sel">Open {{B}}</div>
          <div>Quick picker <span class="k">Ctrl+Shift+Space</span></div>
          <div>Recent <span class="k">›</span></div>
          <hr>
          <div>Pause capture <span class="k">›</span></div>
          <div>Start with Windows <span class="k">✓</span></div>
        </div>
      </div>

      <div class="tile span-2">
        <span class="ico">{{i:shieldcheck}}</span>
        <h3>Safe by default</h3>
        <p>Password managers are ignored. One-time codes, API keys, private keys and card numbers are masked, kept out of search and sync, and deleted after an hour.</p>
      </div>
      <div class="tile span-2">
        <span class="ico">{{i:zap}}</span>
        <h3>Light and fast</h3>
        <p>A Rust core on SQLite with trigram full-text search. Classifying a clip takes well under a millisecond. The app uses your system's web view, not a bundled browser.</p>
      </div>
      <div class="tile span-2">
        <span class="ico">{{i:qr}}</span>
        <h3>Pair with a QR code</h3>
        <p>Scan once and your devices share one history over your Wi-Fi, end-to-end encrypted. No account, no cloud.</p>
        <a class="more" href="#devices">Cross-device sync →</a>
      </div>
    </div>
  </div>
</section>

<section class="section" id="devices">
  <div class="wrap">
    <div class="section-head center">
      <span class="eyebrow">Cross-platform, cross-device</span>
      <h2 class="h2">Any device to any device.</h2>
      <p class="lede">Each platform gets its own app on the same Rust core. Every app is fully useful on its own; pair another device when you have one and what you copy on one shows up in the history of the others.</p>
    </div>
    <div class="flows">
      <div class="flow">
        <div class="flow-track" aria-hidden="true">
          <span class="device"><span class="dev-ico">{{i:windows}}</span>Windows</span>
          <span class="wire"><span class="lock">{{i:lock}}</span><span class="packet k-text">{{i:text}}</span></span>
          <span class="device"><span class="dev-ico">{{i:android}}</span>Android</span>
        </div>
        <h3>Copy on Windows, paste on your phone</h3>
        <p>An address from an email on your laptop is in your phone's {{B}} a moment later. Tap it to copy, then paste anywhere.</p>
        <span class="chip chip-accent">Android app arrives with v0.1</span>
      </div>
      <div class="flow">
        <div class="flow-track" aria-hidden="true">
          <span class="device"><span class="dev-ico">{{i:android}}</span>Android</span>
          <span class="wire"><span class="lock">{{i:lock}}</span><span class="packet k-image" style="animation-delay:-1.1s">{{i:image}}</span></span>
          <span class="device"><span class="dev-ico">{{i:apple}}</span>macOS</span>
        </div>
        <h3>An image from your phone on your Mac</h3>
        <p>Screenshots and photos sync as full images (thumbnails first), so they're there when you sit down at your computer.</p>
        <span class="chip chip-warn">Android v0.1 · macOS experimental</span>
      </div>
      <div class="flow">
        <div class="flow-track" aria-hidden="true">
          <span class="device"><span class="dev-ico">{{i:linux}}</span>Linux</span>
          <span class="wire"><span class="lock">{{i:lock}}</span><span class="packet k-code" style="animation-delay:-2.2s">{{i:code}}</span></span>
          <span class="device"><span class="dev-ico">{{i:windows}}</span>Windows</span>
        </div>
        <h3>Code from Linux, pasted on Windows</h3>
        <p>That command from your Linux box is one hotkey away on your Windows laptop, still tagged with its language.</p>
        <span class="chip chip-ok">Works today on your local network</span>
      </div>
    </div>

    <div class="split" style="margin-top:88px">
      <div>
        <span class="eyebrow">Sync</span>
        <h3 class="h2" style="font-size:clamp(26px,3.2vw,36px)">One scan to pair. Encrypted before it leaves.</h3>
        <ul class="checklist" style="margin-top:20px">
          <li>""" + CHECK + r"""<span><b>QR pairing with a check code.</b> Your computer shows a one-time code valid for 10 minutes; both devices then show the same 4-digit check code.</span></li>
          <li>""" + CHECK + r"""<span><b>End-to-end encrypted.</b> Every clip is sealed with XChaCha20-Poly1305 on the device that copied it. Keys live in the OS keychain.</span></li>
          <li>""" + CHECK + r"""<span><b>On your network, no cloud.</b> A desktop acts as the hub on your Wi-Fi. No account, no server of ours, nothing leaves your home or office.</span></li>
          <li>""" + CHECK + r"""<span><b>Secrets stay put.</b> Sensitive clips are never synced. Synced clips land in history, never silently on your clipboard.</span></li>
          <li>""" + TODO + r"""<span><b>Same network for now.</b> Both devices need to be on the same Wi-Fi or LAN while they sync; changes made while apart catch up when they meet again.</span></li>
        </ul>
        <p style="margin-top:20px"><a class="btn" href="{{R}}docs/sync.html">{{i:book}}Sync guide</a></p>
      </div>
      <div class="window">""" + shot("pairing", "Settings, Devices section: sync is on and ready on port 47821, with a pairing QR code that expires in 9:57 and a Copy link button.", w=1200, h=719) + r"""</div>
    </div>

    <h3 class="h3" style="margin:88px 0 16px">Platform support</h3>
    <div class="table-wrap">
      <table class="matrix">
        <thead><tr><th scope="col">Platform</th><th scope="col">Status</th><th scope="col">Capture</th><th scope="col">Paste-back</th><th scope="col">Sync</th></tr></thead>
        <tbody>
          <tr><th scope="row">Windows 10 / 11</th><td><span class="status-dot ok"></span>Supported</td><td>Automatic</td><td>Yes</td><td>Hub or client</td></tr>
          <tr><th scope="row">Linux</th><td><span class="status-dot beta"></span>Beta</td><td>X11; Wayland with data-control (KDE, Sway, Hyprland). GNOME is limited.</td><td>X11 only</td><td>Hub or client</td></tr>
          <tr><th scope="row">Android</th><td><span class="status-dot soon"></span>Coming in v0.1</td><td>Automatic via accessibility, plus tile and share menu</td><td>Tap to copy</td><td>Client</td></tr>
          <tr><th scope="row">macOS</th><td><span class="status-dot exp"></span>Experimental</td><td>Not implemented yet</td><td>Not yet</td><td>Hub or client (untested)</td></tr>
          <tr><th scope="row">iOS</th><td><span class="status-dot"></span>Later</td><td colspan="3" class="m">Share extension and shortcuts are on the roadmap.</td></tr>
        </tbody>
      </table>
    </div>
  </div>
</section>

<section class="section section-alt">
  <div class="wrap">
    <div class="section-head">
      <span class="eyebrow">Why another clipboard app</span>
      <h2 class="h2">History tools don't sync. Sync tools don't remember.</h2>
      <p class="lede">{{B}} is built to do both, on the mix of devices people actually own. Here's an honest comparison, including what isn't there yet.</p>
    </div>
    <div class="table-wrap">
      <table class="compare">
        <thead><tr><th scope="col"><span class="sr-only">Feature</span></th><th scope="col">Built-in history<br><span class="m">Win+V and similar</span></th><th scope="col">History apps<br><span class="m">one device</span></th><th scope="col">Clipboard sync<br><span class="m">phone links, bridges</span></th><th scope="col" class="us">{{B}}</th></tr></thead>
        <tbody>
          <tr><th scope="row">Keeps everything, searchable</th><td class="n">—</td><td class="y">✓</td><td class="n">—</td><td class="us y">✓</td></tr>
          <tr><th scope="row">Sorted by type automatically</th><td class="n">—</td><td class="m">some</td><td class="n">—</td><td class="us y">✓</td></tr>
          <tr><th scope="row">Timeline and calendar heatmap</th><td class="n">—</td><td class="m">rare</td><td class="n">—</td><td class="us y">✓</td></tr>
          <tr><th scope="row">Smart paste actions</th><td class="n">—</td><td class="m">some</td><td class="n">—</td><td class="us y">✓</td></tr>
          <tr><th scope="row">Laptop ↔ phone</th><td class="m">with an account</td><td class="n">—</td><td class="y">✓</td><td class="us m">Android in v0.1</td></tr>
          <tr><th scope="row">Windows + Linux + Android</th><td class="n">—</td><td class="m">some</td><td class="m">some</td><td class="us m">✓ (Android in v0.1)</td></tr>
          <tr><th scope="row">End-to-end encrypted, open source</th><td class="n">—</td><td class="m">some</td><td class="m">some</td><td class="us y">✓</td></tr>
          <tr><th scope="row">No account needed</th><td class="y">✓</td><td class="y">✓</td><td class="m">some</td><td class="us y">✓</td></tr>
        </tbody>
      </table>
    </div>
  </div>
</section>

<section class="section" id="privacy">
  <div class="wrap split">
    <div>
      <span class="eyebrow">Privacy</span>
      <h2 class="h2">Your clipboard is yours.</h2>
      <p class="lede" style="margin-bottom:24px">A clipboard manager sees passwords, private messages and keys. {{B}} is designed so that nobody else does.</p>
      <ul class="checklist">
        <li>""" + CHECK + r"""<span><b>Local-first.</b> Your history is a folder on your device. Every app works fully offline and alone.</span></li>
        <li>""" + CHECK + r"""<span><b>No telemetry, no analytics, no account.</b> Nothing phones home. Read every line on GitHub.</span></li>
        <li>""" + CHECK + r"""<span><b>Encrypted sync.</b> Only ciphertext crosses the network, and only to devices you paired.</span></li>
        <li>""" + CHECK + r"""<span><b>Password managers excluded</b> by default, plus any app you add.</span></li>
        <li>""" + TODO + r"""<span><b>On the roadmap:</b> encrypting the on-device database at rest. Today it's protected by your OS account, like your browser history.</span></li>
      </ul>
      <p style="margin-top:20px"><a class="btn" href="{{R}}docs/privacy-security.html">{{i:shield}}Privacy &amp; security in detail</a></p>
    </div>
    <div class="cipher-card" role="img" aria-label="Example of a sync envelope as it travels: only an event id, sender id, timestamp, nonce and ciphertext are visible.">
      <div class="head">{{i:lock}}What the network sees</div>
      <div class="row"><span class="lbl">event_id</span><span class="val">01JBX4Q8M7T2ZK9R3D5F6H0WNC</span></div>
      <div class="row"><span class="lbl">device_id</span><span class="val">01JB7Y2C4E8G6K1M3P5R7T9VXZ</span></div>
      <div class="row"><span class="lbl">ts</span><span class="val">1761998112404</span></div>
      <div class="row"><span class="lbl">nonce</span><span class="val enc">p3Xq9vRk2LmT8wYz1aBc4dEf6gHi7jKl</span></div>
      <div class="row"><span class="lbl">ct</span><span class="val enc">tK8vQ2mZ7nR4xW1pL9sY3cJ6hF0dG5bA2eU8iO4qT7wN1mV3kX6zS9rP2yL5…</span></div>
      <div class="row"><span class="lbl">your clip</span><span class="val">never visible in transit</span></div>
    </div>
  </div>
</section>

<section class="section section-alt">
  <div class="wrap-narrow">
    <div class="section-head center">
      <span class="eyebrow">FAQ</span>
      <h2 class="h2">Questions, answered.</h2>
    </div>
    <div class="faq">
      <details><summary>Is {{B}} free?</summary><div class="a"><p>Yes. It's open source under MIT or Apache-2.0, with no account and no paid tier.</p></div></details>
      <details><summary>Does my clipboard leave my computer?</summary><div class="a"><p>Only if you pair another device, and then only as ciphertext to that device over your local network. There is no {{B}} server and no telemetry.</p></div></details>
      <details><summary>When is the Android app coming?</summary><div class="a"><p>With v0.1.0, the first alpha. Capture on Android works through an accessibility service plus a Quick Settings tile and the share menu; it will be a sideloaded APK at first. See <a href="{{R}}docs/android.html">Android setup</a>.</p></div></details>
      <details><summary>Do I need two devices?</summary><div class="a"><p>No. Every app is fully useful on one device: history, search, the picker and smart actions all work offline. Pairing only adds devices when you have them.</p></div></details>
      <details><summary>Is it safe to copy passwords?</summary><div class="a"><p>Copies from password managers are ignored by default. Anything that looks like a one-time code, API key, private key or card number is masked, never searchable, never synced, and deleted after an hour (you can change that).</p></div></details>
    </div>
    <p class="center" style="margin-top:28px"><a class="btn btn-ghost" href="{{R}}docs/faq.html">All questions {{i:arrow}}</a></p>
  </div>
</section>

<section class="final">
  <div class="wrap">
    <span class="logo-big">{{LOGO}}</span>
    <h2 class="h2">Start keeping your trail.</h2>
    <p class="lede">Free, open source, and private by design. Watch the repo to hear the moment v0.1 ships.</p>
    <div class="hero-cta">
      <a class="btn btn-primary btn-lg" href="{{R}}download.html" data-os-cta>{{i:download}}<span data-os-cta-label>Download {{B}}</span></a>
      <a class="btn btn-lg" href="{{REPO}}">{{i:star}}Star on GitHub</a>
    </div>
  </div>
</section>
"""

# ------------------------------------------------------------------ download
def slot(asset, label, primary=False, soon=None, missing=None):
    attrs = f'data-asset="{asset}" data-label="{label}"'
    if primary: attrs += " data-primary"
    if soon: attrs += f' data-soon="{soon}"'
    if missing: attrs += f' data-missing="{missing}"'
    return (f'<div class="asset skeleton" {attrs}><span class="a-main"><span class="a-name">{label}</span>'
            f'<span class="a-meta">Checking GitHub…</span></span></div>')

DOWNLOAD = r"""
<section class="page-head hero" style="padding-bottom:24px">
  <div class="wrap">
    <span class="eyebrow">Download</span>
    <h1 class="h1" style="font-size:clamp(36px,5vw,56px);margin-top:12px">Get {{B}}</h1>
    <p class="lede">Free and open source. Pick your platform below; the one you're on is highlighted. Builds are pre-release and not code-signed yet, so read the install notes for your system.</p>
  </div>
</section>

<section style="padding:8px 0 72px">
  <div class="wrap">
    <div class="dl-status" id="dl-status" aria-live="polite" aria-busy="true">
      <span class="ico">{{i:tag}}</span>
      <div class="txt"><b>Checking for the latest release…</b><span>Asking GitHub which version is newest.</span></div>
    </div>
    <noscript><div class="dl-status warn"><span class="ico">{{i:info}}</span><div class="txt"><b>JavaScript is off</b><span>Open the <a href="{{REPO}}/releases">Releases page on GitHub</a> to download.</span></div></div></noscript>

    <div class="os-grid">
      <article class="os-card" data-os="windows" id="windows">
        <span class="chip chip-accent detected-tag">Your system</span>
        <div class="os-head"><span class="os-logo">{{i:windows}}</span><div><h2>Windows</h2><span class="os-sub">Windows 10 and 11 · x64</span></div></div>
        <p>Installs for your user only, no admin rights needed. Starts in the tray; the picker opens with <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>Space</kbd>.</p>
        <div class="assets">""" + slot("win-exe", "Installer (.exe)", primary=True) + slot("win-msi", "MSI package (.msi)") + r"""</div>
      </article>
      <article class="os-card" data-os="linux" id="linux">
        <span class="chip chip-accent detected-tag">Your system</span>
        <div class="os-head"><span class="os-logo">{{i:linux}}</span><div><h2>Linux <span class="chip chip-muted">Beta</span></h2><span class="os-sub">x86_64 · Debian/Ubuntu or any distro</span></div></div>
        <p>Use the <code>.deb</code> on Debian, Ubuntu and derivatives; the AppImage runs almost anywhere. Paste-back works on X11 only.</p>
        <div class="assets">""" + slot("linux-deb", "Debian / Ubuntu (.deb)", primary=True) + slot("linux-appimage", "AppImage") + r"""</div>
      </article>
      <article class="os-card" data-os="macos" id="macos">
        <span class="chip chip-accent detected-tag">Your system</span>
        <div class="os-head"><span class="os-logo">{{i:apple}}</span><div><h2>macOS <span class="chip chip-warn">Experimental</span></h2><span class="os-sub">Universal (Apple silicon and Intel)</span></div></div>
        <p>Built by CI but untested, and clipboard capture isn't implemented on macOS yet. Try it only if you want to help get it there.</p>
        <div class="assets">""" + slot("mac-dmg", "Disk image (.dmg)", soon="Arrives with v0.1.0 (experimental)") + r"""</div>
      </article>
      <article class="os-card" data-os="android" id="android">
        <span class="chip chip-accent detected-tag">Your system</span>
        <div class="os-head"><span class="os-logo">{{i:android}}</span><div><h2>Android <span class="chip chip-accent">Coming in v0.1</span></h2><span class="os-sub">Sideloaded APK · Android 10+</span></div></div>
        <p>Automatic capture through an accessibility service, a Quick Settings tile and the share menu. Pairs with your computer by QR. Being built now.</p>
        <div class="assets">""" + slot("android-apk", "Android app (.apk)", soon="Coming with v0.1.0", missing="Not in this release yet") + r"""<a class="asset" href="{{R}}docs/android.html">{{i:book}}<span class="a-main"><span class="a-name">Android setup guide</span><span class="a-meta">Samsung and other phones, step by step</span></span></a></div>
      </article>
      <article class="os-card" data-os="ios" id="ios" style="grid-column:1/-1">
        <div class="os-head"><span class="os-logo">{{i:ios}}</span><div><h2>iOS <span class="chip chip-muted">Later</span></h2><span class="os-sub">Not available yet</span></div></div>
        <p style="margin-bottom:0">iOS doesn't let apps read the clipboard in the background, so it will start with a share extension and Shortcuts. It's on the <a href="{{REPO}}/blob/main/ROADMAP.md">roadmap</a> after Android.</p>
      </article>
    </div>
  </div>
</section>

<section class="section-tight section-alt" id="cli">
  <div class="wrap">
    <div class="section-head" style="margin-bottom:32px">
      <span class="eyebrow">Command line</span>
      <h2 class="h2" style="font-size:clamp(26px,3.4vw,38px)">Install from the terminal</h2>
      <p class="lede">The <code>clipd</code> CLI captures, searches and syncs from a terminal or script. Package managers come later.</p>
    </div>
    <div class="install-grid">
      <div class="install-card">
        <h3>{{i:package}}Cargo <span class="chip chip-ok">Works now</span></h3>
        <p>Builds the <code>clipd</code> CLI from source. Needs Rust 1.96+; on Linux also <code>libxcb1-dev</code>.</p>
        <pre><code>cargo install --locked --git https://github.com/abhijitdalal26/cliptrail clipd-cli</code></pre>
      </div>
      <div class="install-card">
        <h3>{{i:windows}}Package managers <span class="chip chip-muted">Later</span></h3>
        <p>Listings for Windows and Linux package managers (such as winget and Scoop) are planned after the first stable release. Until then, use the installers above.</p>
      </div>
      <div class="install-card">
        <h3>{{i:download}}Installers <span class="chip chip-accent">v0.1.0</span></h3>
        <p>Every release attaches the desktop installers and prebuilt <code>clipd</code> archives. Use the buttons above or the <a href="{{REPO}}/releases">Releases page</a>.</p>
      </div>
    </div>
    <div class="os-grid" style="margin-top:16px">
      <article class="os-card">
        <div class="os-head"><span class="os-logo">{{i:terminal}}</span><div><h2>CLI binaries</h2><span class="os-sub">Prebuilt <code>clipd</code>, attached to each release</span></div></div>
        <div class="assets" style="margin-top:16px">""" + slot("cli-win", "clipd for Windows x64 (.zip)") + slot("cli-linux", "clipd for Linux x86_64 (.tar.gz)") + r"""</div>
      </article>
      <article class="os-card">
        <div class="os-head"><span class="os-logo">{{i:code}}</span><div><h2>Build from source</h2><span class="os-sub">Rust 1.96, Node 22 and pnpm</span></div></div>
        <p>Everything builds from the repository today, including the desktop app. Step by step in the docs.</p>
        <div class="assets"><a class="asset" href="{{R}}docs/index.html#build-from-source">{{i:book}}<span class="a-main"><span class="a-name">Build instructions</span><span class="a-meta">Windows, Linux and the CLI</span></span></a><a class="asset" href="{{REPO}}">{{i:github}}<span class="a-main"><span class="a-name">Source on GitHub</span><span class="a-meta">abhijitdalal26/cliptrail</span></span></a></div>
      </article>
    </div>
  </div>
</section>

<section class="section-tight" id="install-notes">
  <div class="wrap">
    <div class="section-head" style="margin-bottom:32px">
      <span class="eyebrow">Install notes</span>
      <h2 class="h2" style="font-size:clamp(26px,3.4vw,38px)">Before you install</h2>
      <p class="lede">Pre-release builds aren't code-signed yet, so your OS will ask you to confirm. Here's what to expect.</p>
    </div>
    <div class="notes">
      <div class="note-card">
        <h3>{{i:windows}}Windows: SmartScreen</h3>
        <p>Because the installer isn't signed yet, Windows shows <strong>"Windows protected your PC"</strong>.</p>
        <ol>
          <li>Click <strong>More info</strong>.</li>
          <li>Click <strong>Run anyway</strong>.</li>
          <li>The first time you turn on sync, allow {{B}} through the firewall on <strong>Private networks</strong>.</li>
        </ol>
      </div>
      <div class="note-card">
        <h3>{{i:linux}}Linux: .deb or AppImage</h3>
        <ul>
          <li><strong>.deb</strong>: <code>sudo apt install ./{{B}}_*.deb</code> pulls in <code>libxdo3</code> for paste-back.</li>
          <li><strong>AppImage</strong>: <code>chmod +x {{B}}_*.AppImage</code>, then run it.</li>
          <li><strong>Wayland</strong>: capture works on compositors with data-control (KDE Plasma, Sway, Hyprland). GNOME lacks it, so {{B}} only sees copies while an X11 app has focus. Paste-back is X11-only: on Wayland the picker copies and you press <kbd>Ctrl</kbd> <kbd>V</kbd>.</li>
        </ul>
      </div>
      <div class="note-card">
        <h3>{{i:apple}}macOS: Gatekeeper</h3>
        <ol>
          <li>Open the <code>.dmg</code> and drag {{B}} to Applications.</li>
          <li>On first launch macOS blocks the unsigned app. Open <strong>System Settings → Privacy &amp; Security</strong> and click <strong>Open Anyway</strong>.</li>
          <li>Paste-back will need <strong>Accessibility</strong> permission once it's implemented on macOS.</li>
        </ol>
      </div>
      <div class="note-card">
        <h3>{{i:android}}Android: sideloading</h3>
        <p>When v0.1 ships, install the APK from the release (allow "Install unknown apps" for your browser or file manager), then follow the in-app checklist: accessibility service, restricted settings, battery, tile. Samsung needs a few extra steps.</p>
        <p style="margin:0"><a href="{{R}}docs/android.html">Android setup guide →</a></p>
      </div>
    </div>

    <h3 class="h3" style="margin:56px 0 14px">System requirements</h3>
    <div class="table-wrap">
      <table class="req-table">
        <tbody>
          <tr><td>Windows</td><td>Windows 10 or 11, 64-bit (x64). Uses the Microsoft Edge WebView2 runtime, which Windows 11 includes; the installer fetches it on older systems.</td></tr>
          <tr><td>Linux</td><td>x86_64 with WebKitGTK 4.1 (Ubuntu 22.04 or newer, Debian 12, Fedora 38+). X11 for paste-back; Wayland capture needs a data-control compositor.</td></tr>
          <tr><td>macOS</td><td>Experimental universal build. Capture and paste-back are not implemented yet.</td></tr>
          <tr><td>Android</td><td>Android 10 or newer (arm64), coming with v0.1. Tested first on Samsung One UI.</td></tr>
          <tr><td>Sync</td><td>Devices on the same local network (Wi-Fi or LAN) that can reach the hub on TCP port 47821.</td></tr>
        </tbody>
      </table>
    </div>
    <p class="muted" style="font-size:14px;margin-top:16px">Checksums: releases don't publish SHA-256 files yet; signed builds and checksums are planned. Every release is built by GitHub Actions from the tagged source (<a href="{{REPO}}/blob/main/.github/workflows/release.yml">release workflow</a>).</p>
  </div>
</section>
"""

CHANGELOG = r"""
<section class="page-head">
  <div class="wrap">
    <span class="eyebrow">Changelog</span>
    <h1 class="h1" style="font-size:clamp(36px,5vw,56px);margin-top:12px">What's new</h1>
    <p class="lede">Everything that's landed, newest first. Releases are published on <a href="{{REPO}}/releases">GitHub</a>; the full history is in the <a href="{{REPO}}/commits/main">commit log</a>.</p>
  </div>
</section>
<section style="padding:16px 0 96px">
  <div class="wrap">
    <div class="changelog-entry">
      <div class="changelog-meta">
        <div class="ver">v0.1.0</div>
        <div class="date">Unreleased</div>
        <span class="chip chip-accent">Alpha · in progress</span>
      </div>
      <div class="prose">
        <p>The first alpha: a Windows and Linux desktop app, the <code>clipd</code> CLI and encrypted LAN sync, with the Android app being built for the same release.</p>

        <div class="cl-group">
          <h3>Encrypted sync and smart actions <a class="commit" href="{{REPO}}/commit/e9c3d7b">e9c3d7b</a></h3>
          <ul>
            <li>The desktop app hosts a sync hub on your local network (HTTP + WebSocket, port 47821). Devices pair by QR code: X25519 key exchange, a one-time secret, a matching 4-digit check code, rate-limited.</li>
            <li>Clip events and images travel in XChaCha20-Poly1305 envelopes. Pins and deletes sync too (last writer wins).</li>
            <li>Sensitive clips are never synced; synced clips never touch your OS clipboard on their own; synced writes never echo back.</li>
            <li>Offline outbox, catch-up and reconnect in the client, built to run on Android as well.</li>
            <li><code>clipd sync host | pair | run | devices | revoke | status | reset</code>.</li>
            <li>Settings → Devices with QR pairing, device list and revoke; sync status in the sidebar.</li>
            <li>Smart actions per kind: clean tracking parameters, Markdown link, URL decode; colour as HEX/RGB/HSL; JSON pretty/minify; Markdown code block; dedent; trim, single line, case changes; phone digits; mailto link.</li>
            <li>Actions with live previews in the dashboard; <kbd>Ctrl</kbd> <kbd>Enter</kbd> action list in the picker; optional automatic link cleaning on capture.</li>
          </ul>
        </div>

        <div class="cl-group">
          <h3>Android capture spike <a class="commit" href="{{REPO}}/commit/02cd71b">02cd71b</a></h3>
          <ul>
            <li>A standalone test app to prove copy capture without a custom keyboard: accessibility-triggered capture with a brief focus grab, Quick Settings tile, share target and text-selection action.</li>
            <li>Measures its own capture rate per signal; no network permission; on-device log encrypted with an Android Keystore key.</li>
          </ul>
        </div>

        <div class="cl-group">
          <h3>Release pipeline <a class="commit" href="{{REPO}}/commit/47c44bb">47c44bb</a></h3>
          <ul>
            <li>Tagging <code>v*</code> builds Windows (<code>.exe</code>, <code>.msi</code>), Linux (<code>.deb</code>, AppImage), experimental macOS (<code>.dmg</code>) and CLI archives into a draft release.</li>
          </ul>
        </div>

        <div class="cl-group">
          <h3>Desktop app <a class="commit" href="{{REPO}}/commit/02e5754">02e5754</a></h3>
          <ul>
            <li>Dashboard: sidebar with counts per kind and app, search, a day-grouped timeline, calendar heatmap, a preview pane per kind, copy / copy as plain text / pin / delete with undo, light and dark themes.</li>
            <li>Quick picker on <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>Space</kbd>: <kbd>Enter</kbd> pastes into the previous app, <kbd>Shift</kbd> <kbd>Enter</kbd> pastes plain text, <kbd>Ctrl</kbd> <kbd>1</kbd>–<kbd>9</kbd> quick-paste.</li>
            <li>Tray menu with recent clips, pause capture, start with Windows and settings.</li>
            <li>Settings: rebindable hotkeys, retention, sensitive-clip lifetime, excluded apps.</li>
          </ul>
        </div>

        <div class="cl-group">
          <h3>Core, capture and CLI <a class="commit" href="{{REPO}}/commit/fbf1b1f">fbf1b1f</a></h3>
          <ul>
            <li><code>clipd-core</code>: SQLite store with FTS5 trigram search and versioned migrations; a local classifier for eight kinds (tested on 145 samples); dedup; password-manager exclusion; masked, expiring secrets; image thumbnails.</li>
            <li>Windows capture: event-driven watcher, rich text, images and files, honours the "don't record this" clipboard flags password managers set, paste-back.</li>
            <li>Linux capture: X11 and Wayland data-control backends.</li>
            <li><code>clipd</code>: <code>watch</code>, <code>list</code>, <code>search</code>, <code>show</code>, <code>stats</code>, <code>pin</code>, <code>delete</code>, <code>prune</code>, <code>ingest-text</code>.</li>
          </ul>
        </div>

        <div class="cl-group">
          <h3>Still to come in v0.1</h3>
          <ul>
            <li>The Android app (capture, tile, share menu, pairing).</li>
            <li>The first tagged release with installers.</li>
          </ul>
          <p class="muted">Coming later: sync across networks, on-device database encryption, paste stack, snippets, OCR. See the <a href="{{REPO}}/blob/main/ROADMAP.md">roadmap</a>.</p>
        </div>
      </div>
    </div>
  </div>
</section>
"""

PRIVACY = r"""
<section class="legal">
  <div class="wrap">
    <article class="prose">
      <span class="eyebrow">Privacy policy</span>
      <h1>Privacy</h1>
      <p class="updated">Last updated 1 November 2026</p>
      <p class="doc-lede">Short version: the {{B}} app collects nothing, and this website doesn't track you.</p>

      <h2 id="app">The {{B}} app</h2>
      <ul>
        <li><strong>No data collection.</strong> The apps and the <code>clipd</code> CLI have no telemetry, analytics or crash reporting, and they never contact a {{B}} server, because there isn't one.</li>
        <li><strong>Update checks are optional and off until you choose.</strong> On the desktop app, {{B}} can look for a new version, but only if you turn on <em>Check for updates automatically</em> (it asks once, and you can say no) or click <em>Check for updates</em> in Settings. A check is a plain request for a small file on GitHub Releases (<code>github.com</code>), so GitHub sees your IP address like any download; no information about you, your device or your clips is sent. Updates are signed, verified before they run, and never installed without your confirmation. The Android app never checks.</li>
        <li><strong>Your history stays on your devices.</strong> Clips are stored in a data folder on each device (see <a href="docs/data.html">Data, backup &amp; uninstall</a>). You can delete any clip, or the whole folder, at any time.</li>
        <li><strong>Sync is between your own devices.</strong> If you pair devices, clips travel directly between them over your local network, encrypted end to end. Only devices you paired can read them.</li>
        <li><strong>No account.</strong> There is nothing to sign up for and no personal information to give.</li>
        <li><strong>Sensitive clips</strong> (one-time codes, keys, card numbers) are masked, kept out of search and sync, and deleted after a short time. Copies from password managers are not saved at all.</li>
      </ul>
      <p>The Android app, when it ships, uses an accessibility service only to notice that you copied something and read the clipboard at that moment. It doesn't read screen content for any other purpose.</p>

      <h2 id="website">This website</h2>
      <ul>
        <li><strong>No cookies, analytics, trackers, ads or third-party fonts.</strong> Everything is served from GitHub Pages.</li>
        <li><strong>GitHub Pages</strong> hosts the site, so GitHub receives the usual request information (such as your IP address) when you visit. See <a href="https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement">GitHub's privacy statement</a>.</li>
        <li><strong>The Download page</strong> asks the GitHub API (<code>api.github.com</code>) which release is newest, directly from your browser, so it can link the right files. GitHub's privacy statement applies to that request. The answer is cached in your browser's session storage for 15 minutes and discarded when you close the tab.</li>
        <li><strong>Theme choice.</strong> If you switch between light and dark, the choice is saved in your browser's local storage. It never leaves your device.</li>
      </ul>

      <h2 id="contact">Questions</h2>
      <p>Open an issue on <a href="https://github.com/abhijitdalal26/cliptrail/issues">GitHub</a>. For anything security-related, please follow the <a href="security.html">security policy</a> instead.</p>
    </article>
  </div>
</section>
"""

SECURITY = r"""
<section class="legal">
  <div class="wrap">
    <article class="prose">
      <span class="eyebrow">Security</span>
      <h1>Security</h1>
      <p class="doc-lede">A clipboard manager sits in the most sensitive data path on your machine. This is what {{B}} promises today, what it doesn't yet, and how to report a problem.</p>

      <div class="callout warn">{{i:warn}}<div><p><strong>Pre-release software.</strong> {{B}} hasn't had an independent security audit. Sync is end-to-end encrypted; on-device database encryption is on the roadmap.</p></div></div>

      <h2 id="model">The model in brief</h2>
      <div class="table-wrap">
        <table>
          <thead><tr><th scope="col">Where</th><th scope="col">Protection today</th></tr></thead>
          <tbody>
            <tr><td>On your device</td><td>A SQLite database and image files in your user's data folder, protected by your OS account. Not yet encrypted at rest.</td></tr>
            <tr><td>Between devices</td><td>Every clip and image is sealed with XChaCha20-Poly1305 under a key only your paired devices hold. Requests are authenticated with HMAC-SHA256; the device token itself never travels.</td></tr>
            <tr><td>Pairing</td><td>X25519 key agreement over a one-time QR secret (10-minute, single-use, rate-limited) plus a 4-digit check code shown on both devices.</td></tr>
            <tr><td>Keys</td><td>Windows Credential Manager, macOS Keychain; elsewhere (Linux, Android) a file readable only by your user or the app.</td></tr>
            <tr><td>Secrets you copy</td><td>Password-manager copies are skipped. OTPs, API keys, JWTs, private keys and card numbers are masked, not indexed, never synced and deleted after 1 hour by default.</td></tr>
            <tr><td>Logs</td><td>Clip content is never logged or put in error messages; only ids, kinds and sizes.</td></tr>
          </tbody>
        </table>
      </div>
      <p>The full explanation, including what an attacker on your network can and can't see, is in <a href="docs/privacy-security.html">Privacy &amp; security</a>.</p>

      <h2 id="report">Reporting a vulnerability</h2>
      <p>Please <strong>don't open a public issue</strong> for security problems.</p>
      <ul>
        <li>Report privately through <a href="https://github.com/abhijitdalal26/cliptrail/security/advisories/new">GitHub private vulnerability reporting</a>.</li>
        <li>Read the repository's <a href="https://github.com/abhijitdalal26/cliptrail/blob/main/SECURITY.md">SECURITY.md</a> for scope and severity.</li>
        <li>We aim to acknowledge reports within 72 hours.</li>
      </ul>
      <h3 id="critical">Treated as critical</h3>
      <ul>
        <li>Clip content reaching another party in plaintext, or reaching a device that wasn't paired.</li>
        <li>Secrets or clip bodies written to logs.</li>
        <li>Key material leaving the device it belongs to.</li>
        <li>Seeing someone else's history.</li>
      </ul>
    </article>
  </div>
</section>
"""

NOTFOUND = r"""
<section class="notfound">
  <div class="wrap">
    <span class="code">404</span>
    <h1 class="h2" style="margin-top:18px">This page isn't on the trail.</h1>
    <p class="lede" style="margin:0 auto 28px">The link may be old, or the page moved when the docs were reorganised.</p>
    <div class="hero-cta" style="margin-top:0">
      <a class="btn btn-primary" href="{{R}}index.html">Go home</a>
      <a class="btn" href="{{R}}docs/index.html">{{i:book}}Docs</a>
      <a class="btn btn-ghost" href="{{R}}download.html">{{i:download}}Download</a>
    </div>
  </div>
</section>
"""

def build(b):
    b.page("index.html", "{{B}}: your clipboard, everywhere, forever",
           "Open-source clipboard history for Windows and Linux, with Android coming in v0.1: remembers everything you copy, sorts it by type, and syncs end-to-end encrypted between your devices.",
           INDEX, active=None)
    b.page("download.html", "Download",
           "Download {{B}} for Windows, Linux and macOS (experimental), and the clipd command-line tool. Android arrives with v0.1.",
           DOWNLOAD, active="download", extra_head='\n<script src="assets/download.js" defer></script>')
    b.page("changelog.html", "Changelog", "What's new in {{B}}, release by release.", CHANGELOG, active="changelog")
    b.page("privacy.html", "Privacy policy", "{{B}} collects nothing. No telemetry, no account, and no cookies or analytics on this website.", PRIVACY)
    b.page("security.html", "Security", "{{B}}'s security model and how to report a vulnerability privately.", SECURITY)
    # 404 is served at any depth, so it uses absolute links.
    nf_root = "/cliptrail/"
    doc = b.head("Page not found", "This page doesn't exist.", "404.html", nf_root) + "<body>\n" + b.header(nf_root, None) + \
        f'<main id="main">\n{b.expand(NOTFOUND, nf_root)}\n</main>\n' + b.footer(nf_root) + "</body>\n</html>\n"
    doc = doc.replace('<meta name="viewport"', '<meta name="robots" content="noindex">\n<meta name="viewport"')
    b.write("404.html", doc)
    # robots + sitemap
    pages = ["", "download.html", "changelog.html", "privacy.html", "security.html"] + \
            ["docs/" + (f if f != "index.html" else "") for _, items in b.DOCS for f, _, _ in items]
    urls = "\n".join(f"  <url><loc>{b.BASE}{p}</loc><lastmod>2026-11-01</lastmod></url>" for p in pages)
    b.write("sitemap.xml", f'<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n{urls}\n</urlset>\n')
    b.write("robots.txt", f"User-agent: *\nAllow: /\n\nSitemap: {b.BASE}sitemap.xml\n")

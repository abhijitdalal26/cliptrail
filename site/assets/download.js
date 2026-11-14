// Download page: find the newest published release (pre-releases included) on
// GitHub and wire every installer button to its asset. The only network
// request on this site: api.github.com, straight from your browser.
(function () {
  "use strict";
  var REPO = "abhijitdalal26/cliptrail";
  var API = "https://api.github.com/repos/" + REPO + "/releases?per_page=15";
  var RELEASES = "https://github.com/" + REPO + "/releases";
  var CACHE_KEY = "cliptrail-releases-v1";
  var CACHE_MS = 15 * 60 * 1000;

  // Slot id -> test on the lowercased asset name.
  var MATCH = {
    "win-exe": function (n) { return /-setup\.exe$/.test(n); },
    "win-msi": function (n) { return /\.msi$/.test(n); },
    "linux-deb": function (n) { return /\.deb$/.test(n); },
    "linux-appimage": function (n) { return /\.appimage$/.test(n); },
    "mac-dmg": function (n) { return /\.dmg$/.test(n); },
    "android-apk": function (n) { return /\.apk$/.test(n); },
    "cli-win": function (n) { return /^clipd-.*-x86_64-pc-windows-msvc\.zip$/.test(n); },
    "cli-linux": function (n) { return /^clipd-.*-x86_64-unknown-linux-gnu\.tar\.gz$/.test(n); }
  };

  var ICON = {
    down: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/></svg>',
    clock: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>',
    tag: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 2H2v10l9.29 9.29a1 1 0 0 0 1.41 0l8.59-8.59a1 1 0 0 0 0-1.41Z"/><circle cx="7" cy="7" r="1.5"/></svg>',
    warn: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z"/><path d="M12 9v4M12 17h.01"/></svg>',
    eye: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/></svg>',
    star: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m12 2 3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z"/></svg>'
  };

  function esc(s) {
    return String(s).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }
  function size(bytes) {
    if (!bytes && bytes !== 0) return "";
    var mb = bytes / (1024 * 1024);
    return mb >= 1 ? mb.toFixed(mb >= 100 ? 0 : 1) + " MB" : Math.max(1, Math.round(bytes / 1024)) + " KB";
  }
  function day(iso) {
    try { return new Date(iso).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" }); } catch (e) { return ""; }
  }

  function readCache() {
    try {
      var c = JSON.parse(sessionStorage.getItem(CACHE_KEY) || "null");
      if (c && Date.now() - c.t < CACHE_MS && Array.isArray(c.data)) return c.data;
    } catch (e) { /* no storage */ }
    return null;
  }
  function writeCache(data) {
    try { sessionStorage.setItem(CACHE_KEY, JSON.stringify({ t: Date.now(), data: data })); } catch (e) { /* no storage */ }
  }

  // Keep only what the page needs (small cache, no surprises).
  function slim(list) {
    return list.map(function (r) {
      return {
        draft: !!r.draft, prerelease: !!r.prerelease, tag: r.tag_name, name: r.name, url: r.html_url,
        date: r.published_at || r.created_at,
        assets: (r.assets || []).map(function (a) { return { name: a.name, size: a.size, url: a.browser_download_url }; })
      };
    });
  }

  function pick(list) {
    var pub = list.filter(function (r) { return !r.draft; });
    pub.sort(function (a, b) { return new Date(b.date) - new Date(a.date); });
    return pub[0] || null;
  }

  var statusEl = document.getElementById("dl-status");
  function setStatus(kind, icon, title, text, actions) {
    if (!statusEl) return;
    statusEl.className = "dl-status" + (kind === "warn" ? " warn" : "");
    statusEl.setAttribute("aria-busy", "false");
    statusEl.innerHTML =
      '<span class="ico">' + icon + "</span>" +
      '<div class="txt"><b>' + title + "</b><span>" + text + "</span></div>" +
      (actions ? '<div class="acts">' + actions + "</div>" : "");
  }

  function slotHTML(slot, asset, release, state) {
    var label = esc(slot.getAttribute("data-label") || "");
    var primary = slot.hasAttribute("data-primary");
    if (asset) {
      var meta = [esc(asset.name), size(asset.size)].filter(Boolean).join(" · ");
      return '<a class="asset' + (primary ? " primary" : "") + '" href="' + esc(asset.url) + '" download>' + ICON.down +
        '<span class="a-main"><span class="a-name">' + label + '</span><span class="a-meta">' + meta + "</span></span></a>";
    }
    var why = state === "none" ? (slot.getAttribute("data-soon") || "Arrives with v0.1.3")
      : state === "error" ? "Couldn't reach GitHub · see the Releases page"
      : (slot.getAttribute("data-missing") || "Not in " + esc(release ? release.tag : "this release"));
    var href = state === "error" ? RELEASES : null;
    var inner = ICON.clock + '<span class="a-main"><span class="a-name">' + label + '</span><span class="a-meta">' + why + "</span></span>";
    return href
      ? '<a class="asset unavailable" href="' + href + '">' + inner + "</a>"
      : '<div class="asset unavailable" aria-disabled="true">' + inner + "</div>";
  }

  function fill(release, state) {
    document.querySelectorAll("[data-asset]").forEach(function (slot) {
      var test = MATCH[slot.getAttribute("data-asset")];
      var asset = null;
      if (release && test) {
        release.assets.forEach(function (a) {
          var n = a.name.toLowerCase();
          if (!asset && !/\.sig$/.test(n) && test(n)) asset = a;
        });
      }
      slot.outerHTML = slotHTML(slot, asset, release, state);
    });
    document.querySelectorAll("[data-release-version]").forEach(function (el) {
      el.textContent = release ? release.tag : "v0.1.3";
    });
  }

  function render(list) {
    var r = pick(list);
    if (!r) {
      setStatus("info", ICON.clock, "First release coming soon",
        "v0.1.3 (alpha) is being prepared. Watch the repository to hear when it lands, or build from source today.",
        '<a class="btn btn-sm btn-primary" href="https://github.com/' + REPO + '/subscription">' + ICON.eye + "Watch releases</a>" +
        '<a class="btn btn-sm" href="https://github.com/' + REPO + '">' + ICON.star + "Star</a>" +
        '<a class="btn btn-sm btn-ghost" href="docs/index.html#build-from-source">Build from source</a>');
      fill(null, "none");
      return;
    }
    setStatus("ok", ICON.tag,
      esc(r.name || r.tag) + (r.prerelease ? ' <span class="chip chip-warn">Pre-release</span>' : ""),
      "Released " + esc(day(r.date)) + " · " + r.assets.length + " files",
      '<a class="btn btn-sm" href="' + esc(r.url) + '">Release notes</a>' +
      '<a class="btn btn-sm btn-ghost" href="' + RELEASES + '">All releases</a>');
    fill(r, "ok");
  }

  function fail(rateLimited) {
    setStatus("warn", ICON.warn,
      rateLimited ? "GitHub's API limit was reached" : "Couldn't load the release list",
      "The downloads are still on GitHub: open the Releases page and pick the file for your system.",
      '<a class="btn btn-sm btn-primary" href="' + RELEASES + '">Open Releases on GitHub</a>');
    fill(null, "error");
  }

  // Highlight the visitor's platform.
  var os = window.siteOS || "unknown";
  var card = document.querySelector('.os-card[data-os="' + os + '"]');
  if (card) {
    card.classList.add("detected");
    var grid = card.parentNode;
    if (grid && grid.firstElementChild !== card) grid.insertBefore(card, grid.firstElementChild);
  }

  var cached = readCache();
  if (cached) { render(cached); return; }
  if (!window.fetch) { fail(false); return; }
  fetch(API, { headers: { Accept: "application/vnd.github+json" } })
    .then(function (res) {
      if (res.status === 403 || res.status === 429) {
        var err = new Error("rate");
        err.rate = res.headers.get("x-ratelimit-remaining") === "0" || res.status === 429;
        throw err;
      }
      if (res.status === 404) return [];
      if (!res.ok) throw new Error("http " + res.status);
      return res.json();
    })
    .then(function (json) {
      var list = slim(Array.isArray(json) ? json : []);
      writeCache(list);
      render(list);
    })
    .catch(function (e) { fail(!!(e && e.rate)); });
})();

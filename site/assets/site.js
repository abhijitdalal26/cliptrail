// Project website: theme toggle, menus, OS detection, copy buttons, docs nav/TOC.
// No tracking, no network requests (the Download page has its own script).
(function () {
  "use strict";
  var root = document.documentElement;
  var THEME_KEY = "cliptrail-theme";

  function store(key, value) {
    try { if (value == null) localStorage.removeItem(key); else localStorage.setItem(key, value); } catch (e) { /* storage blocked */ }
  }
  function systemDark() {
    return window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches;
  }
  function isDark() {
    var t = root.getAttribute("data-theme");
    return t ? t === "dark" : systemDark();
  }

  // ---- theme toggle: flips the effective theme; matching the system clears the override
  document.querySelectorAll("[data-theme-toggle]").forEach(function (btn) {
    var label = function () { btn.setAttribute("aria-label", isDark() ? "Switch to light theme" : "Switch to dark theme"); };
    label();
    btn.addEventListener("click", function () {
      var next = isDark() ? "light" : "dark";
      if ((next === "dark") === systemDark()) { root.removeAttribute("data-theme"); store(THEME_KEY, null); }
      else { root.setAttribute("data-theme", next); store(THEME_KEY, next); }
      label();
    });
  });

  // ---- header: border once scrolled, mobile menu
  var header = document.querySelector(".site-header");
  if (header) {
    var onScroll = function () { header.classList.toggle("scrolled", window.scrollY > 8); };
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
  }
  var menuBtn = document.querySelector("[data-menu-btn]");
  var mobileNav = document.getElementById("mobile-nav");
  if (menuBtn && mobileNav) {
    var setMenu = function (open) {
      mobileNav.classList.toggle("open", open);
      menuBtn.setAttribute("aria-expanded", String(open));
      menuBtn.setAttribute("aria-label", open ? "Close menu" : "Open menu");
    };
    menuBtn.addEventListener("click", function () { setMenu(!mobileNav.classList.contains("open")); });
    document.addEventListener("keydown", function (e) { if (e.key === "Escape") setMenu(false); });
    mobileNav.addEventListener("click", function (e) { if (e.target.closest("a")) setMenu(false); });
    window.addEventListener("resize", function () { if (window.innerWidth > 960) setMenu(false); });
  }

  // ---- OS detection (shared with download.js through window.siteOS)
  function detectOS() {
    var uad = navigator.userAgentData;
    var p = (uad && uad.platform) || "";
    var ua = navigator.userAgent || "";
    if (/android/i.test(p) || /Android/.test(ua)) return "android";
    if (/iPhone|iPad|iPod/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1)) return "ios";
    if (/windows/i.test(p) || /Windows|Win64|Win32/.test(ua)) return "windows";
    if (/mac/i.test(p) || /Macintosh|Mac OS X/.test(ua)) return "macos";
    if (/linux|chrome os|cros/i.test(p) || /Linux|X11|CrOS/.test(ua)) return "linux";
    return "unknown";
  }
  var os = detectOS();
  window.siteOS = os;
  root.setAttribute("data-os", os);
  var BRAND = root.getAttribute("data-brand") || "";
  var OS_LABEL = { windows: "Windows", macos: "macOS", linux: "Linux", android: "Android", ios: "iOS" };
  var CTA = {
    windows: "Download for Windows",
    linux: "Download for Linux",
    macos: "Download for macOS",
    android: "Get the Android app",
    ios: "See all downloads",
    unknown: ("Download " + BRAND).trim()
  };
  document.querySelectorAll("[data-os-cta]").forEach(function (el) {
    var span = el.querySelector("[data-os-cta-label]") || el;
    span.textContent = CTA[os];
    var href = el.getAttribute("href");
    if (href && href.indexOf("#") === -1 && OS_LABEL[os]) el.setAttribute("href", href + "#" + os);
  });

  // ---- copy buttons on code blocks
  if (navigator.clipboard) {
    document.querySelectorAll("pre").forEach(function (pre) {
      if (pre.hasAttribute("data-nocopy")) return;
      var code = pre.querySelector("code");
      if (!code) return;
      var b = document.createElement("button");
      b.type = "button";
      b.className = "copy-btn";
      b.textContent = "Copy";
      b.setAttribute("aria-label", "Copy code to clipboard");
      b.addEventListener("click", function () {
        // Leave out prompt markers and comment-only lines.
        var clone = code.cloneNode(true);
        clone.querySelectorAll(".p").forEach(function (n) { n.remove(); });
        var text = clone.textContent.split("\n").filter(function (l) { return !/^\s*#( |$)/.test(l) || code.classList.contains("keep-comments"); }).join("\n").trim();
        navigator.clipboard.writeText(text).then(function () {
          b.textContent = "Copied";
          setTimeout(function () { b.textContent = "Copy"; }, 1600);
        }, function () { b.textContent = "Press Ctrl+C"; });
      });
      pre.appendChild(b);
    });
  }

  // ---- docs: search filter over the sidebar
  var search = document.getElementById("docs-search");
  var nav = document.querySelector(".docs-nav");
  if (search && nav) {
    var items = Array.prototype.slice.call(nav.querySelectorAll("li"));
    var empty = nav.querySelector(".empty");
    var filter = function () {
      var q = search.value.trim().toLowerCase();
      var shown = 0;
      items.forEach(function (li) {
        var hay = (li.textContent + " " + (li.getAttribute("data-keywords") || "")).toLowerCase();
        var hit = !q || q.split(/\s+/).every(function (w) { return hay.indexOf(w) !== -1; });
        li.hidden = !hit;
        if (hit) shown++;
      });
      nav.querySelectorAll("h2").forEach(function (h) {
        var ul = h.nextElementSibling;
        var any = ul && Array.prototype.some.call(ul.children, function (li) { return !li.hidden; });
        h.hidden = !any;
      });
      if (empty) empty.style.display = shown ? "none" : "block";
      var side = document.querySelector(".docs-side");
      if (q && side) side.classList.add("open");
    };
    search.addEventListener("input", filter);
    search.addEventListener("keydown", function (e) {
      if (e.key === "Enter") {
        var first = nav.querySelector("li:not([hidden]) a");
        if (first) window.location.href = first.getAttribute("href");
      } else if (e.key === "Escape") { search.value = ""; filter(); search.blur(); }
    });
    document.addEventListener("keydown", function (e) {
      var t = e.target;
      if (e.key === "/" && !(t && (t.isContentEditable || /INPUT|TEXTAREA|SELECT/.test(t.tagName)))) {
        e.preventDefault();
        search.focus();
      }
    });
  }
  var docsMenu = document.querySelector("[data-docs-menu]");
  if (docsMenu) {
    docsMenu.addEventListener("click", function () {
      var side = document.querySelector(".docs-side");
      var open = !side.classList.contains("open");
      side.classList.toggle("open", open);
      docsMenu.setAttribute("aria-expanded", String(open));
    });
  }

  // ---- docs: heading anchors + on-page TOC with scrollspy
  var prose = document.querySelector(".docs-main .prose");
  var toc = document.getElementById("toc-list");
  if (prose) {
    var heads = Array.prototype.slice.call(prose.querySelectorAll("h2[id], h3[id]"));
    heads.forEach(function (h) {
      var a = document.createElement("a");
      a.className = "anchor";
      a.href = "#" + h.id;
      a.setAttribute("aria-label", "Link to this section");
      a.textContent = "#";
      h.appendChild(a);
    });
    if (toc && heads.length) {
      var links = {};
      heads.forEach(function (h) {
        var li = document.createElement("li");
        var a = document.createElement("a");
        a.href = "#" + h.id;
        a.textContent = h.firstChild ? h.firstChild.textContent.trim() : h.textContent;
        if (h.tagName === "H3") a.className = "sub";
        li.appendChild(a);
        toc.appendChild(li);
        links[h.id] = a;
      });
      if ("IntersectionObserver" in window) {
        var current = null;
        var io = new IntersectionObserver(function (entries) {
          entries.forEach(function (en) {
            if (en.isIntersecting) {
              if (current) current.classList.remove("active");
              current = links[en.target.id];
              if (current) current.classList.add("active");
            }
          });
        }, { rootMargin: "-80px 0px -70% 0px" });
        heads.forEach(function (h) { io.observe(h); });
      }
    } else if (toc) {
      var box = toc.closest(".toc-box");
      if (box) box.hidden = true;
    }
  }
})();

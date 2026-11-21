"""Generates the static website in site/ (plain HTML; nothing to build when serving).

    python site/_src/build.py

Content lives in pages.py (home, download, changelog, legal) and docs/*.html.
The product name appears once, in BRAND below; content uses the {{B}} token.
To rename the product: change BRAND, retake the screenshots (they show the
app), rerun this script, and regenerate assets/og.png (see og.html).
Tokens: {{B}} brand, {{R}} relative root, {{REPO}}, {{LOGO}}, {{i:icon}},
{{SHORTCUTS:array}} (rows parsed from apps/desktop/src/dashboard/ShortcutsSheet.tsx),
{{AIAPPS}} (logo row of the AI apps Settings → AI assistants connects).
"""
import os, re, sys, html

HERE = os.path.dirname(os.path.abspath(__file__))
SITE = os.path.dirname(HERE)
BRAND = "ClipTrail"
BASE = "https://abhijitdalal26.github.io/cliptrail/"
REPO = "https://github.com/abhijitdalal26/cliptrail"

def svg(body, fill=False, sw="2"):
    if fill:
        return f'<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">{body}</svg>'
    return f'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="{sw}" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">{body}</svg>'

I = {
 "github": svg('<path d="M12 .5a11.5 11.5 0 0 0-3.64 22.41c.58.1.79-.25.79-.56v-2c-3.2.7-3.88-1.37-3.88-1.37-.52-1.33-1.28-1.68-1.28-1.68-1.04-.71.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.76 2.7 1.25 3.36.96.1-.75.4-1.25.73-1.54-2.55-.29-5.24-1.28-5.24-5.69 0-1.26.45-2.29 1.19-3.09-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.77 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.59.23 2.76.11 3.05.74.8 1.19 1.83 1.19 3.09 0 4.42-2.7 5.4-5.26 5.68.41.36.78 1.06.78 2.14v3.17c0 .31.21.67.8.56A11.5 11.5 0 0 0 12 .5Z"/>', fill=True),
 "sun": svg('<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/>').replace("<svg ", '<svg class="sun" ', 1),
 "moon": svg('<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>').replace("<svg ", '<svg class="moon" ', 1),
 "menu": svg('<path d="M4 7h16M4 12h16M4 17h16"/>'),
 "download": svg('<path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/>'),
 "arrow": svg('<path d="M5 12h14"/><path d="m13 6 6 6-6 6"/>'),
 "book": svg('<path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20"/>'),
 "star": svg('<path d="m12 2 3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z"/>'),
 "eye": svg('<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/>'),
 "check": svg('<path d="M20 6 9 17l-5-5"/>', sw="3"),
 "dash": svg('<path d="M6 12h12"/>', sw="3"),
 "lock": svg('<rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>'),
 "shield": svg('<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10Z"/>'),
 "shieldcheck": svg('<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10Z"/><path d="m9 12 2 2 4-4"/>'),
 "zap": svg('<path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z"/>'),
 "history": svg('<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5M12 7v5l3 2"/>'),
 "tag": svg('<path d="M12 2H2v10l9.29 9.29a1 1 0 0 0 1.41 0l8.59-8.59a1 1 0 0 0 0-1.41Z"/><circle cx="7" cy="7" r="1.5"/>'),
 "keyboard": svg('<rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10"/>'),
 "sparkles": svg('<path d="m12 3-1.9 5.8a2 2 0 0 1-1.3 1.3L3 12l5.8 1.9a2 2 0 0 1 1.3 1.3L12 21l1.9-5.8a2 2 0 0 1 1.3-1.3L21 12l-5.8-1.9a2 2 0 0 1-1.3-1.3Z"/>'),
 "calendar": svg('<rect x="3" y="4" width="18" height="18" rx="2"/><path d="M16 2v4M8 2v4M3 10h18M8 14h.01M12 14h.01M16 14h.01M8 18h.01M12 18h.01"/>'),
 "laptop": svg('<rect x="3" y="4" width="18" height="12" rx="1.5"/><path d="M2 20h20"/>'),
 "monitor": svg('<rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/>'),
 "phone": svg('<rect x="6" y="2" width="12" height="20" rx="2.5"/><path d="M11 18h2"/>'),
 "devices": svg('<rect x="2" y="4" width="13" height="10" rx="1.5"/><path d="M5 18h7"/><rect x="17" y="8" width="5" height="12" rx="1.2"/>'),
 "tray": svg('<path d="M4 6h16M4 12h16M4 18h7"/><path d="m15 18 2 2 4-4"/>'),
 "search": svg('<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>'),
 "copy": svg('<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>'),
 "link": svg('<path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>'),
 "code": svg('<path d="m16 18 6-6-6-6M8 6l-6 6 6 6"/>'),
 "text": svg('<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6M8 13h8M8 17h5"/>'),
 "contact": svg('<circle cx="12" cy="8" r="4"/><path d="M4 21a8 8 0 0 1 16 0"/>'),
 "color": svg('<circle cx="13.5" cy="6.5" r="1"/><circle cx="17.5" cy="10.5" r="1"/><circle cx="8.5" cy="7.5" r="1"/><circle cx="6.5" cy="12.5" r="1"/><path d="M12 2a10 10 0 0 0 0 20c.9 0 1.7-.8 1.7-1.7 0-.4-.2-.8-.4-1.1-.3-.3-.4-.7-.4-1.1 0-.9.8-1.7 1.7-1.7h2a5.6 5.6 0 0 0 5.6-5.6C22 6 17.5 2 12 2Z"/>'),
 "file": svg('<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>'),
 "image": svg('<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21"/>'),
 "secret": svg('<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10Z"/><path d="M12 8v4M12 16h.01"/>'),
 "windows": svg('<path d="M3 5.5 10.5 4.4v7.1H3zM12 4.2 21 3v8.5h-9zM3 12.5h7.5v7.1L3 18.5zM12 12.5h9V21l-9-1.2z"/>', fill=True),
 "apple": svg('<path d="M16.4 12.7c0-2.4 2-3.6 2.1-3.7-1.1-1.7-2.9-1.9-3.5-1.9-1.5-.2-2.9.9-3.7.9-.8 0-1.9-.9-3.2-.8-1.6 0-3.1 1-4 2.4-1.7 3-.4 7.4 1.2 9.8.8 1.2 1.8 2.5 3 2.4 1.2 0 1.7-.8 3.1-.8 1.5 0 1.9.8 3.2.8 1.3 0 2.1-1.2 2.9-2.4.9-1.3 1.3-2.7 1.3-2.8 0 0-2.4-1-2.4-3.9ZM14.1 5.6c.7-.8 1.1-1.9 1-3-1 0-2.1.7-2.8 1.5-.6.7-1.2 1.8-1 2.9 1 .1 2.1-.6 2.8-1.4Z"/>', fill=True),
 "linux": svg('<path d="M12 2.5c-2.1 0-3.4 1.8-3.4 4.1 0 1.3.3 2.1-.4 3.5-1.1 1.9-3 4.3-3 6.9 0 1 .3 1.7.9 2.2L5.4 21h3.4c.9.5 2 .8 3.2.8s2.3-.3 3.2-.8h3.4l-.7-1.8c.6-.5.9-1.2.9-2.2 0-2.6-1.9-5-3-6.9-.7-1.4-.4-2.2-.4-3.5 0-2.3-1.3-4.1-3.4-4.1Z"/><path d="M10.2 6.6h.01M13.8 6.6h.01"/><path d="m10.6 8.6 1.4.9 1.4-.9"/>'),
 "android": svg('<path d="M4 18a8 8 0 0 1 16 0z"/><path d="M7.6 10.6 5.6 7.2M16.4 10.6l2-3.4"/><path d="M9 14.6h.01M15 14.6h.01"/>', sw="2"),
 "ios": svg('<rect x="6" y="2" width="12" height="20" rx="3"/><path d="M11 5h2"/>'),
 "terminal": svg('<path d="m4 17 6-6-6-6M12 19h8"/>'),
 "info": svg('<circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/>'),
 "warn": svg('<path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z"/><path d="M12 9v4M12 17h.01"/>'),
 "wifi": svg('<path d="M5 12.55a11 11 0 0 1 14.08 0M1.42 9a16 16 0 0 1 21.16 0M8.53 16.11a6 6 0 0 1 6.95 0M12 20h.01"/>'),
 "qr": svg('<rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><path d="M14 14h3v3M21 14v.01M14 21h.01M17 21h4v-4"/>'),
 "external": svg('<path d="M15 3h6v6M10 14 21 3M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>'),
 "edit": svg('<path d="M12 20h9"/><path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z"/>'),
 "bug": svg('<path d="m8 2 1.88 1.88M14.12 3.88 16 2M9 7.13v-1a3 3 0 1 1 6 0v1"/><path d="M12 20c-3.3 0-6-2.7-6-6v-3a4 4 0 0 1 4-4h4a4 4 0 0 1 4 4v3c0 3.3-2.7 6-6 6Z"/><path d="M12 20v-9M6.53 9C4.6 8.8 3 7.1 3 5M6 13H2M3 21c0-2.1 1.7-3.9 3.8-4M20.97 5c0 2.1-1.6 3.8-3.5 4M22 13h-4M17.2 17c2.1.1 3.8 1.9 3.8 4"/>'),
 "pin": svg('<path d="M12 17v5M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"/>'),
 "trash": svg('<path d="M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>'),
 "cpu": svg('<rect x="4" y="4" width="16" height="16" rx="2"/><rect x="9" y="9" width="6" height="6"/><path d="M15 2v2M15 20v2M2 15h2M2 9h2M20 15h2M20 9h2M9 2v2M9 20v2"/>'),
 "eyeoff": svg('<path d="M9.88 9.88a3 3 0 1 0 4.24 4.24M10.73 5.08A10.4 10.4 0 0 1 12 5c7 0 10 7 10 7a13.2 13.2 0 0 1-1.67 2.68M6.61 6.61A13.5 13.5 0 0 0 2 12s3 7 10 7a9.7 9.7 0 0 0 5.39-1.61M2 2l20 20"/>'),
 "package": svg('<path d="M16.5 9.4 7.5 4.21M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/><path d="M3.27 6.96 12 12.01l8.73-5.05M12 22.08V12"/>'),
 "cloudoff": svg('<path d="m2 2 20 20M5.78 5.78A7 7 0 0 0 9 19h8.5a4.5 4.5 0 0 0 1.31-.2M21.53 16.5A4.5 4.5 0 0 0 17.5 10h-1.79A7 7 0 0 0 9.3 5.07"/>'),
 "users": svg('<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75"/>'),
}

LOGO = ('<svg viewBox="0 0 1024 1024" aria-hidden="true">'
        '<defs><linearGradient id="lg-bg" x1="80" y1="40" x2="980" y2="980" gradientUnits="userSpaceOnUse">'
        '<stop offset="0" stop-color="#2F93FD"/><stop offset="0.34" stop-color="#455EFE"/>'
        '<stop offset="0.62" stop-color="#6A36FC"/><stop offset="1" stop-color="#8A2AFB"/>'
        '</linearGradient></defs>'
        # squircle (same superellipse outline as the app icon)
        '<path fill="url(#lg-bg)" d="M 1016.00 512.00 L 1015.74 647.90 L 1014.94 695.60 L 1013.62 730.80 L 1011.77 759.65 L 1009.38 784.45 L 1006.45 806.36 L 1002.97 826.04 L 998.95 843.94 L 994.36 860.33 L 989.20 875.43 L 983.47 889.40 L 977.14 902.34 L 970.20 914.36 L 962.63 925.52 L 954.40 935.89 L 945.50 945.50 L 935.89 954.40 L 925.52 962.63 L 914.36 970.20 L 902.34 977.14 L 889.40 983.47 L 875.43 989.20 L 860.33 994.36 L 843.94 998.95 L 826.04 1002.97 L 806.36 1006.45 L 784.45 1009.38 L 759.65 1011.77 L 730.80 1013.62 L 695.60 1014.94 L 647.90 1015.74 L 512.00 1016.00 L 376.10 1015.74 L 328.40 1014.94 L 293.20 1013.62 L 264.35 1011.77 L 239.55 1009.38 L 217.64 1006.45 L 197.96 1002.97 L 180.06 998.95 L 163.67 994.36 L 148.57 989.20 L 134.60 983.47 L 121.66 977.14 L 109.64 970.20 L 98.48 962.63 L 88.11 954.40 L 78.50 945.50 L 69.60 935.89 L 61.37 925.52 L 53.80 914.36 L 46.86 902.34 L 40.53 889.40 L 34.80 875.43 L 29.64 860.33 L 25.05 843.94 L 21.03 826.04 L 17.55 806.36 L 14.62 784.45 L 12.23 759.65 L 10.38 730.80 L 9.06 695.60 L 8.26 647.90 L 8.00 512.00 L 8.26 376.10 L 9.06 328.40 L 10.38 293.20 L 12.23 264.35 L 14.62 239.55 L 17.55 217.64 L 21.03 197.96 L 25.05 180.06 L 29.64 163.67 L 34.80 148.57 L 40.53 134.60 L 46.86 121.66 L 53.80 109.64 L 61.37 98.48 L 69.60 88.11 L 78.50 78.50 L 88.11 69.60 L 98.48 61.37 L 109.64 53.80 L 121.66 46.86 L 134.60 40.53 L 148.57 34.80 L 163.67 29.64 L 180.06 25.05 L 197.96 21.03 L 217.64 17.55 L 239.55 14.62 L 264.35 12.23 L 293.20 10.38 L 328.40 9.06 L 376.10 8.26 L 512.00 8.00 L 647.90 8.26 L 695.60 9.06 L 730.80 10.38 L 759.65 12.23 L 784.45 14.62 L 806.36 17.55 L 826.04 21.03 L 843.94 25.05 L 860.33 29.64 L 875.43 34.80 L 889.40 40.53 L 902.34 46.86 L 914.36 53.80 L 925.52 61.37 L 935.89 69.60 L 945.50 78.50 L 954.40 88.11 L 962.63 98.48 L 970.20 109.64 L 977.14 121.66 L 983.47 134.60 L 989.20 148.57 L 994.36 163.67 L 998.95 180.06 L 1002.97 197.96 L 1006.45 217.64 L 1009.38 239.55 L 1011.77 264.35 L 1013.62 293.20 L 1014.94 328.40 L 1015.74 376.10 Z"/>'
        # clipboard glyph (white silhouette from the mono mark)
        '<g fill="#fff">'
        '<path d="M 314.00 250.00 H 662.00 A 86.00 86.00 0 0 1 748.00 336.00 V 724.00 A 86.00 86.00 0 0 1 662.00 810.00 H 314.00 A 86.00 86.00 0 0 1 228.00 724.00 V 336.00 A 86.00 86.00 0 0 1 314.00 250.00 Z"/>'
        '<path d="M 348.00 214.00 H 628.00 A 34.00 34.00 0 0 1 662.00 248.00 V 280.00 A 34.00 34.00 0 0 1 628.00 314.00 H 348.00 A 34.00 34.00 0 0 1 314.00 280.00 V 248.00 A 34.00 34.00 0 0 1 348.00 214.00 Z"/>'
        '<path d="M 457.00 134.00 H 519.00 A 57.00 57.00 0 0 1 576.00 191.00 V 191.00 A 57.00 57.00 0 0 1 519.00 248.00 H 457.00 A 57.00 57.00 0 0 1 400.00 191.00 V 191.00 A 57.00 57.00 0 0 1 457.00 134.00 Z"/>'
        '</g>'
        # punched-out details (line slots, clip hole, clock badge) painted in the gradient
        '<g fill="#5A16E4">'
        '<circle cx="488.0" cy="190" r="22"/>'
        '<rect x="314.0" y="398.0" width="318.0" height="30.0" rx="15.0"/>'
        '<rect x="314.0" y="484.0" width="318.0" height="30.0" rx="15.0"/>'
        '<rect x="314.0" y="570.0" width="318.0" height="30.0" rx="15.0"/>'
        '</g>'
        # clock badge: white disc + purple history arrow & hands
        '<circle cx="708" cy="748" r="186" fill="#fff"/>'
        '<circle cx="708" cy="748" r="172" fill="#F4F1FE"/>'
        '<path d="M 610.27 712.43 A 104.00 104.00 0 1 1 630.71 817.59" fill="none" stroke="#5A16E4" stroke-width="34" stroke-linecap="round"/>'
        '<path d="M 619.98 852.42 L 602.96 786.76 L 666.47 810.56 Z" fill="#5A16E4"/>'
        '<path d="M 721.00 693.00 L 721.00 749.30 L 720.01 754.27 L 717.19 758.49 L 712.97 761.31 L 708.00 762.30 L 703.03 761.31 L 698.81 758.49 L 695.99 754.27 L 695.00 749.30 L 695.00 693.00 L 695.99 688.03 L 698.81 683.81 L 703.03 680.99 L 708.00 680.00 L 712.97 680.99 L 717.19 683.81 L 720.01 688.03 L 721.00 693.00 Z" fill="#5A16E4"/>'
        '<path d="M 731.81 776.76 L 700.37 758.61 L 696.56 755.26 L 694.32 750.71 L 693.99 745.65 L 695.62 740.85 L 698.96 737.04 L 703.51 734.79 L 708.57 734.46 L 713.37 736.09 L 744.81 754.24 L 748.62 757.59 L 750.87 762.14 L 751.20 767.20 L 749.57 772.00 L 746.22 775.81 L 741.68 778.06 L 736.61 778.39 L 731.81 776.76 Z" fill="#5A16E4"/>'
        '</svg>')

def expand(text, R):
    text = text.replace("{{B}}", BRAND).replace("{{R}}", R).replace("{{REPO}}", REPO).replace("{{LOGO}}", LOGO)
    return re.sub(r"\{\{i:([a-z]+)\}\}", lambda m: I[m.group(1)], text)

def head(title, desc, path, R, extra=""):
    canonical = BASE + ("" if path == "index.html" else path.replace("index.html", ""))
    title, desc = title.replace("{{B}}", BRAND), desc.replace("{{B}}", BRAND)
    full_title = title if title.startswith(BRAND) else f"{title} · {BRAND}"
    return f"""<!doctype html>
<html lang="en" data-brand="{BRAND}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{html.escape(full_title)}</title>
<meta name="description" content="{html.escape(desc)}">
<link rel="canonical" href="{canonical}">
<meta property="og:type" content="website">
<meta property="og:site_name" content="{BRAND}">
<meta property="og:title" content="{html.escape(full_title)}">
<meta property="og:description" content="{html.escape(desc)}">
<meta property="og:url" content="{canonical}">
<meta property="og:image" content="{BASE}assets/og.png">
<meta property="og:image:width" content="1200">
<meta property="og:image:height" content="630">
<meta property="og:image:alt" content="{BRAND}: your clipboard, everywhere, forever.">
<meta name="twitter:card" content="summary_large_image">
<meta name="theme-color" content="#fafaf9" media="(prefers-color-scheme: light)">
<meta name="theme-color" content="#0b0d12" media="(prefers-color-scheme: dark)">
<meta name="color-scheme" content="light dark">
<link rel="icon" href="{R}favicon.svg" type="image/svg+xml">
<link rel="icon" href="{R}favicon.ico" sizes="any">
<link rel="icon" type="image/png" sizes="32x32" href="{R}assets/favicon-32x32.png">
<link rel="icon" type="image/png" sizes="16x16" href="{R}assets/favicon-16x16.png">
<link rel="apple-touch-icon" href="{R}assets/apple-touch-icon.png">
<link rel="manifest" href="{R}assets/site.webmanifest">
<link rel="stylesheet" href="{R}assets/style.css">
<script>try{{var t=localStorage.getItem("cliptrail-theme");if(t==="light"||t==="dark")document.documentElement.setAttribute("data-theme",t)}}catch(e){{}}document.documentElement.classList.add("js")</script>
<script src="{R}assets/site.js" defer></script>{extra}
</head>
"""

NAV = [("Features", "index.html#features", "features"), ("Download", "download.html", "download"),
       ("Docs", "docs/index.html", "docs"), ("Changelog", "changelog.html", "changelog")]

def header(R, active, wide=False):
    links = "".join(
        '<a href="{0}{1}"{2}>{3}</a>'.format(R, href, ' aria-current="page"' if key == active else '', label)
        for label, href, key in NAV)
    mlinks = "".join(f'<a href="{R}{href}">{label}</a>' for label, href, key in NAV)
    return f"""<a class="skip" href="#main">Skip to content</a>
<header class="site-header{' wide' if wide else ''}">
  <div class="wrap">
    <a class="brand" href="{R}index.html" aria-label="{BRAND} home">{LOGO}{BRAND}<span class="tag-alpha">alpha</span></a>
    <nav class="nav" aria-label="Main">{links}</nav>
    <div class="header-actions">
      <a class="icon-btn gh-btn" href="{REPO}" aria-label="{BRAND} on GitHub">{I['github']}</a>
      <button class="icon-btn theme-toggle" type="button" data-theme-toggle aria-label="Switch theme">{I['sun']}{I['moon']}</button>
      <a class="btn btn-sm btn-primary btn-download" href="{R}download.html">{I['download']}Download</a>
      <button class="icon-btn menu-btn" type="button" data-menu-btn aria-expanded="false" aria-controls="mobile-nav" aria-label="Open menu">{I['menu']}</button>
    </div>
  </div>
</header>
<div class="mobile-nav" id="mobile-nav">{mlinks}<a href="{REPO}">GitHub</a><a class="btn btn-primary" href="{R}download.html">{I['download']}Download</a></div>
"""

def footer(R):
    return f"""<footer class="site-footer">
  <div class="wrap">
    <div class="footer-grid">
      <div class="footer-brand">
        <a class="brand" href="{R}index.html">{LOGO}{BRAND}</a>
        <p>Open-source clipboard history that remembers everything you copy and moves it between your devices, end-to-end encrypted.</p>
      </div>
      <div>
        <h2>Product</h2>
        <ul>
          <li><a href="{R}download.html">Download</a></li>
          <li><a href="{R}index.html#features">Features</a></li>
          <li><a href="{R}changelog.html">Changelog</a></li>
          <li><a href="{REPO}/blob/main/ROADMAP.md">Roadmap</a></li>
        </ul>
      </div>
      <div>
        <h2>Docs</h2>
        <ul>
          <li><a href="{R}docs/index.html">Getting started</a></li>
          <li><a href="{R}docs/shortcuts.html">Keyboard shortcuts</a></li>
          <li><a href="{R}docs/sync.html">Sync your devices</a></li>
          <li><a href="{R}docs/cli.html">Command line</a></li>
          <li><a href="{R}docs/ai-assistants.html">AI assistants</a></li>
          <li><a href="{R}docs/troubleshooting.html">Troubleshooting</a></li>
        </ul>
      </div>
      <div>
        <h2>Project</h2>
        <ul>
          <li><a href="{REPO}">GitHub</a></li>
          <li><a href="{REPO}/issues/new">Report an issue</a></li>
          <li><a href="{R}security.html">Security</a></li>
          <li><a href="{R}privacy.html">Privacy</a></li>
          <li><a href="{R}code-signing.html">Code signing policy</a></li>
          <li><a href="{REPO}/blob/main/LICENSE">License: MIT</a></li>
        </ul>
      </div>
    </div>
    <div class="footer-bottom">
      <span>© 2026 {BRAND} contributors · MIT licensed (<a href="{REPO}/blob/main/LICENSE">license</a>)</span>
      <span>No cookies, analytics or trackers on this site.</span>
    </div>
  </div>
</footer>
"""

def write(path, content):
    out = os.path.join(SITE, path.replace("/", os.sep))
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)

def page(path, title, desc, body, active=None, extra_head="", body_class=""):
    R = "../" * path.count("/")
    doc = head(title, desc, path, R, extra_head) + f'<body{" class=" + chr(34) + body_class + chr(34) if body_class else ""}>\n' + header(R, active) + \
        f'<main id="main">\n{expand(body, R)}\n</main>\n' + footer(R) + "</body>\n</html>\n"
    write(path, doc)

# ---------------------------------------------------------------- docs
DOCS = [
  ("Get started", [
    ("index.html", "Getting started", "install install-windows install-linux first run build source"),
    ("picker.html", "Quick picker & paste-back", "hotkey paste enter ctrl shift space"),
    ("shortcuts.html", "Keyboard shortcuts", "keys hotkeys keyboard"),
  ]),
  (f"Using {BRAND}", [
    ("kinds.html", "Kinds & classification", "link code text contact color file image sensitive language detect"),
    ("smart-actions.html", "Smart actions", "transform clean url tracking json color rgb hsl markdown dedent"),
    ("search.html", "Search & timeline", "find filter heatmap calendar date app pinned"),
    ("ai-assistants.html", "AI assistants (MCP)", "ai claude cursor mcp agent clipd-mcp chatgpt assistant llm"),
  ]),
  ("Devices", [
    ("sync.html", "Sync your devices", "pair qr hub lan firewall wifi revoke encryption"),
    ("android.html", "Android setup", "samsung phone notification tile share battery apk"),
  ]),
  ("Reference", [
    ("settings.html", "Settings", "preferences options retention exclude theme port"),
    ("cli.html", "Command line (clipd)", "cli terminal clipd commands flags"),
    ("data.html", "Data, backup & uninstall", "folder backup reset remove appdata CLIPTRAIL_DATA_DIR"),
  ]),
  ("Trust", [
    ("privacy-security.html", "Privacy & security", "encryption threat model telemetry password manager"),
  ]),
  ("Help", [
    ("troubleshooting.html", "Troubleshooting", "problem fix not working wayland firewall samsung"),
    ("faq.html", "FAQ", "questions price free"),
  ]),
]
BADGES = {"android.html": '<span class="chip chip-accent">v0.1</span>'}

def docs_nav(current):
    out = ['<nav class="docs-nav" aria-label="Documentation">']
    for group, items in DOCS:
        out.append(f"<h2>{group}</h2><ul>")
        for f, t, kw in items:
            cur = ' aria-current="page"' if f == current else ""
            out.append(f'<li data-keywords="{kw}"><a href="{f}"{cur}><span>{t}</span>{BADGES.get(f, "")}</a></li>')
        out.append("</ul>")
    out.append('<p class="empty">No pages match. Try the <a href="faq.html">FAQ</a>.</p></nav>')
    return "".join(out)

def docs_page(fname, title, desc, body):
    flat = [(f, t) for _, items in DOCS for f, t, _ in items]
    idx = [f for f, _ in flat].index(fname)
    group = next(g for g, items in DOCS if any(f == fname for f, _, _ in items))
    prev_ = flat[idx - 1] if idx > 0 else None
    next_ = flat[idx + 1] if idx + 1 < len(flat) else None
    pager = '<nav class="docs-pager" aria-label="Previous and next page">'
    if prev_: pager += f'<a class="prev" href="{prev_[0]}"><span class="lbl">← Previous</span><span class="ttl">{prev_[1]}</span></a>'
    if next_: pager += f'<a class="next" href="{next_[0]}"><span class="lbl">Next →</span><span class="ttl">{next_[1]}</span></a>'
    pager += "</nav>"
    path = "docs/" + fname
    R = "../"
    side = f"""<aside class="docs-side" aria-label="Docs navigation">
  <div class="docs-search"><label class="sr-only" for="docs-search">Search the docs</label>{I['search']}<input id="docs-search" type="search" placeholder="Search docs" autocomplete="off"><kbd>/</kbd></div>
  <button class="btn btn-sm docs-menu-btn" type="button" data-docs-menu aria-expanded="false">All docs pages<span aria-hidden="true">▾</span></button>
  {docs_nav(fname)}
</aside>"""
    toc = f"""<aside class="docs-toc" aria-label="On this page"><div class="toc-box"><h2>On this page</h2><ul id="toc-list"></ul></div>
  <div class="toc-extra"><a href="{REPO}/blob/main/site/_src/docs/{fname}">{I['edit']}Edit this page</a><a href="{REPO}/issues/new">{I['bug']}Report a problem</a></div></aside>"""
    main = f"""<div class="docs-layout">
{side}
<main class="docs-main" id="main">
  <div class="docs-crumb"><a href="index.html">Docs</a> / {group}</div>
  <article class="prose">
{expand(body, R)}
  </article>
  {pager}
  <p class="docs-edit">Something wrong or missing? <a href="{REPO}/blob/main/site/_src/docs/{fname}">Edit this page on GitHub</a>.</p>
</main>
{toc}
</div>"""
    doc = head(title, desc, path, R) + '<body class="docs-page">\n' + header(R, "docs", wide=True).replace('href="#main"', 'href="#main"') + \
        main + "\n" + footer(R) + "</body>\n</html>\n"
    write(path, doc)

def read_fragment(p):
    with open(p, encoding="utf-8") as f:
        raw = f.read()
    meta, body = raw.split("\n---\n", 1)
    m = dict(line.split(": ", 1) for line in meta.strip().splitlines())
    return m, body

# ------------------------------------------------ shortcuts from the app source
SHEET = os.path.join(os.path.dirname(SITE), "apps", "desktop", "src", "dashboard", "ShortcutsSheet.tsx")

def _combo_html(keys):
    return "".join(f"<kbd>{html.escape(k)}</kbd>" for k in keys)

def shortcut_rows(array_name):
    src = open(SHEET, encoding="utf-8").read()
    m = re.search(r"const " + re.escape(array_name) + r": Entry\[\] = \[(.*?)\n  \];", src, re.S)
    rows = []
    for line in m.group(1).strip().splitlines():
        lm = re.match(r'\s*\["(.*?)", \[(.*)\]\],?$', line)
        if not lm:
            continue
        label, combos_src = lm.groups()
        label = {"This sheet": "Show the shortcuts sheet"}.get(label, label)
        combos = []
        for cm in re.finditer(r'k\("([^"]+)"\)|\[((?:"[^"]*"(?:,\s*)?)+)\]', combos_src):
            if cm.group(1):
                combos.append(cm.group(1).split("+"))
            else:
                combos.append(re.findall(r'"([^"]*)"', cm.group(2)))
        parts = []
        for i, c in enumerate(combos):
            if c == ["…"]:
                parts.append('<span class="or">–</span>')
                continue
            if i > 0 and combos[i - 1] != ["…"]:
                parts.append('<span class="or">or</span>')
            parts.append(_combo_html(c))
        rows.append(f'<tr><td>{html.escape(label)}</td><td><span class="keys">{"".join(parts)}</span></td></tr>')
    return "\n".join(rows)

def shortcuts_table(array_name):
    return ('<div class="table-wrap"><table><thead><tr><th scope="col">Action</th><th scope="col">Keys</th></tr></thead><tbody>\n'
            + shortcut_rows(array_name) + "\n</tbody></table></div>")

# AI apps the one switch connects (Settings → AI assistants). Logos from Simple Icons
# (CC0, simpleicons.org) in each brand's colour; black ones follow the text colour so
# they show in dark mode. LM Studio has no icon there and gets a generic one.
BRAND_PATHS = {
 "claude": "m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z",
 "cursor": "M11.503.131 1.891 5.678a.84.84 0 0 0-.42.726v11.188c0 .3.162.575.42.724l9.609 5.55a1 1 0 0 0 .998 0l9.61-5.55a.84.84 0 0 0 .42-.724V6.404a.84.84 0 0 0-.42-.726L12.497.131a1.01 1.01 0 0 0-.996 0M2.657 6.338h18.55c.263 0 .43.287.297.515L12.23 22.918c-.062.107-.229.064-.229-.06V12.335a.59.59 0 0 0-.295-.51l-9.11-5.257c-.109-.063-.064-.23.061-.23",
 "githubcopilot": "M23.922 16.997C23.061 18.492 18.063 22.02 12 22.02 5.937 22.02.939 18.492.078 16.997A.641.641 0 0 1 0 16.741v-2.869a.883.883 0 0 1 .053-.22c.372-.935 1.347-2.292 2.605-2.656.167-.429.414-1.055.644-1.517a10.098 10.098 0 0 1-.052-1.086c0-1.331.282-2.499 1.132-3.368.397-.406.89-.717 1.474-.952C7.255 2.937 9.248 1.98 11.978 1.98c2.731 0 4.767.957 6.166 2.093.584.235 1.077.546 1.474.952.85.869 1.132 2.037 1.132 3.368 0 .368-.014.733-.052 1.086.23.462.477 1.088.644 1.517 1.258.364 2.233 1.721 2.605 2.656a.841.841 0 0 1 .053.22v2.869a.641.641 0 0 1-.078.256Zm-11.75-5.992h-.344a4.359 4.359 0 0 1-.355.508c-.77.947-1.918 1.492-3.508 1.492-1.725 0-2.989-.359-3.782-1.259a2.137 2.137 0 0 1-.085-.104L4 11.746v6.585c1.435.779 4.514 2.179 8 2.179 3.486 0 6.565-1.4 8-2.179v-6.585l-.098-.104s-.033.045-.085.104c-.793.9-2.057 1.259-3.782 1.259-1.59 0-2.738-.545-3.508-1.492a4.359 4.359 0 0 1-.355-.508Zm2.328 3.25c.549 0 1 .451 1 1v2c0 .549-.451 1-1 1-.549 0-1-.451-1-1v-2c0-.549.451-1 1-1Zm-5 0c.549 0 1 .451 1 1v2c0 .549-.451 1-1 1-.549 0-1-.451-1-1v-2c0-.549.451-1 1-1Zm3.313-6.185c.136 1.057.403 1.913.878 2.497.442.544 1.134.938 2.344.938 1.573 0 2.292-.337 2.657-.751.384-.435.558-1.15.558-2.361 0-1.14-.243-1.847-.705-2.319-.477-.488-1.319-.862-2.824-1.025-1.487-.161-2.192.138-2.533.529-.269.307-.437.808-.438 1.578v.021c0 .265.021.562.063.893Zm-1.626 0c.042-.331.063-.628.063-.894v-.02c-.001-.77-.169-1.271-.438-1.578-.341-.391-1.046-.69-2.533-.529-1.505.163-2.347.537-2.824 1.025-.462.472-.705 1.179-.705 2.319 0 1.211.175 1.926.558 2.361.365.414 1.084.751 2.657.751 1.21 0 1.902-.394 2.344-.938.475-.584.742-1.44.878-2.497Z",
 "windsurf": "M23.55 5.067c-1.2038-.002-2.1806.973-2.1806 2.1765v4.8676c0 .972-.8035 1.7594-1.7597 1.7594-.568 0-1.1352-.286-1.4718-.7659l-4.9713-7.1003c-.4125-.5896-1.0837-.941-1.8103-.941-1.1334 0-2.1533.9635-2.1533 2.153v4.8957c0 .972-.7969 1.7594-1.7596 1.7594-.57 0-1.1363-.286-1.4728-.7658L.4076 5.1598C.2822 4.9798 0 5.0688 0 5.2882v4.2452c0 .2147.0656.4228.1884.599l5.4748 7.8183c.3234.462.8006.8052 1.3509.9298 1.3771.313 2.6446-.747 2.6446-2.0977v-4.893c0-.972.7875-1.7593 1.7596-1.7593h.003a1.798 1.798 0 0 1 1.4718.7658l4.9723 7.0994c.4135.5905 1.05.941 1.8093.941 1.1587 0 2.1515-.9645 2.1515-2.153v-4.8948c0-.972.7875-1.7594 1.7596-1.7594h.194a.22.22 0 0 0 .2204-.2202v-4.622a.22.22 0 0 0-.2203-.2203Z",
 "openai": "M22.2819 9.8211a5.9847 5.9847 0 0 0-.5157-4.9108 6.0462 6.0462 0 0 0-6.5098-2.9A6.0651 6.0651 0 0 0 4.9807 4.1818a5.9847 5.9847 0 0 0-3.9977 2.9 6.0462 6.0462 0 0 0 .7427 7.0966 5.98 5.98 0 0 0 .511 4.9107 6.051 6.051 0 0 0 6.5146 2.9001A5.9847 5.9847 0 0 0 13.2599 24a6.0557 6.0557 0 0 0 5.7718-4.2058 5.9894 5.9894 0 0 0 3.9977-2.9001 6.0557 6.0557 0 0 0-.7475-7.0729zm-9.022 12.6081a4.4755 4.4755 0 0 1-2.8764-1.0408l.1419-.0804 4.7783-2.7582a.7948.7948 0 0 0 .3927-.6813v-6.7369l2.02 1.1686a.071.071 0 0 1 .038.052v5.5826a4.504 4.504 0 0 1-4.4945 4.4944zm-9.6607-4.1254a4.4708 4.4708 0 0 1-.5346-3.0137l.142.0852 4.783 2.7582a.7712.7712 0 0 0 .7806 0l5.8428-3.3685v2.3324a.0804.0804 0 0 1-.0332.0615L9.74 19.9502a4.4992 4.4992 0 0 1-6.1408-1.6464zM2.3408 7.8956a4.485 4.485 0 0 1 2.3655-1.9728V11.6a.7664.7664 0 0 0 .3879.6765l5.8144 3.3543-2.0201 1.1685a.0757.0757 0 0 1-.071 0l-4.8303-2.7865A4.504 4.504 0 0 1 2.3408 7.872zm16.5963 3.8558L13.1038 8.364 15.1192 7.2a.0757.0757 0 0 1 .071 0l4.8303 2.7913a4.4944 4.4944 0 0 1-.6765 8.1042v-5.6772a.79.79 0 0 0-.407-.667zm2.0107-3.0231l-.142-.0852-4.7735-2.7818a.7759.7759 0 0 0-.7854 0L9.409 9.2297V6.8974a.0662.0662 0 0 1 .0284-.0615l4.8303-2.7866a4.4992 4.4992 0 0 1 6.6802 4.66zM8.3065 12.863l-2.02-1.1638a.0804.0804 0 0 1-.038-.0567V6.0742a4.4992 4.4992 0 0 1 7.3757-3.4537l-.142.0805L8.704 5.459a.7948.7948 0 0 0-.3927.6813zm1.0976-2.3654l2.602-1.4998 2.6069 1.4998v2.9994l-2.5974 1.4997-2.6067-1.4997Z",
 "googlegemini": "M11.04 19.32Q12 21.51 12 24q0-2.49.93-4.68.96-2.19 2.58-3.81t3.81-2.55Q21.51 12 24 12q-2.49 0-4.68-.93a12.3 12.3 0 0 1-3.81-2.58 12.3 12.3 0 0 1-2.58-3.81Q12 2.49 12 0q0 2.49-.96 4.68-.93 2.19-2.55 3.81a12.3 12.3 0 0 1-3.81 2.58Q2.49 12 0 12q2.49 0 4.68.96 2.19.93 3.81 2.55t2.55 3.81",
}
AI_APPS = [
 ("Claude Desktop", 'claude', '#D97757'),
 ("Claude Code", 'claude', '#D97757'),
 ("Cursor", 'cursor', None),
 ("VS Code + Copilot", 'githubcopilot', None),
 ("Windsurf", 'windsurf', None),
 ("Codex CLI", 'openai', None),
 ("Gemini CLI", 'googlegemini', '#8E75B2'),
 ("LM Studio", None, None),
]

def ai_apps_row():
    chips = []
    for label, slug, colour in AI_APPS:
        if slug:
            style = f' style="color:{colour}"' if colour else ""
            ico = f'<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"{style}><path d="{BRAND_PATHS[slug]}"/></svg>'
        else:
            ico = I["cpu"]
        chips.append(f'<li class="ai-app">{ico}<span>{html.escape(label)}</span></li>')
    return ('<ul class="ai-apps" aria-label="Works with">' + "".join(chips)
            + '<li class="ai-app more"><span>+ any MCP app</span></li></ul>')

_orig_expand = expand
def expand(text, R):
    if "{{SHORTCUTS:" in text:
        text = re.sub(r"\{\{SHORTCUTS:(\w+)\}\}", lambda m: shortcuts_table(m.group(1)), text)
    if "{{AIAPPS}}" in text:
        text = text.replace("{{AIAPPS}}", ai_apps_row())
    return _orig_expand(text, R)

def write_og_card():
    """_src/og.html: the 1200x630 social card. Screenshot it to assets/og.png
    (e.g. headless Chrome: --window-size=1200,630 --screenshot)."""
    card = f'''<!doctype html><html><head><meta charset="utf-8"><style>
*{{box-sizing:border-box}}body{{margin:0;width:1200px;height:630px;overflow:hidden;font-family:"Segoe UI Variable Display","Segoe UI",system-ui,sans-serif;background:#0b0d12;color:#ecedf1;position:relative}}
.bg{{position:absolute;inset:0;background:radial-gradient(ellipse 60% 70% at 85% 10%,rgba(139,125,255,.35),transparent 60%),radial-gradient(ellipse 50% 60% at 100% 100%,rgba(224,86,155,.18),transparent 60%);}}
.grid{{position:absolute;inset:0;background-image:linear-gradient(rgba(236,237,241,.05) 1px,transparent 1px),linear-gradient(90deg,rgba(236,237,241,.05) 1px,transparent 1px);background-size:48px 48px;-webkit-mask-image:radial-gradient(ellipse 80% 80% at 30% 40%,#000,transparent 80%)}}
.copy{{position:absolute;left:72px;top:84px;width:600px}}
.brand{{display:flex;align-items:center;gap:16px;font-size:34px;font-weight:700;letter-spacing:-.02em}}
.brand svg{{width:56px;height:56px}}
h1{{font-size:68px;line-height:1.03;letter-spacing:-.04em;margin:48px 0 22px;font-weight:760}}
.g{{background:linear-gradient(100deg,#8b7dff 5%,#e0569b 55%,#f59e0b 95%);-webkit-background-clip:text;color:transparent}}
p{{font-size:25px;color:#9aa0ad;margin:0;line-height:1.45}}
.pills{{position:absolute;left:72px;bottom:64px;display:flex;gap:10px}}
.pill{{font-size:19px;padding:7px 16px;border-radius:999px;border:1px solid #2a3040;color:#c9ccd5;background:rgba(19,22,30,.8)}}
.shot{{position:absolute;right:-60px;top:110px;width:560px;border-radius:18px;overflow:hidden;box-shadow:0 40px 80px -20px rgba(0,0,0,.8),0 0 0 1px #2a3040}}
.shot img{{display:block;width:100%}}
</style></head><body><div class="bg"></div><div class="grid"></div>
<div class="copy"><div class="brand">{LOGO.replace('var(--accent)', '#8b7dff')}{BRAND}</div>
<h1>Your clipboard,<br><span class="g">everywhere, forever.</span></h1>
<p>Open-source clipboard history that syncs end-to-end encrypted between your devices.</p></div>
<div class="pills"><span class="pill">Windows</span><span class="pill">Linux</span><span class="pill">macOS</span><span class="pill">Android · v0.1</span><span class="pill">Open source</span></div>
<div class="shot"><img src="../assets/screens/picker-dark.webp"></div>
</body></html>'''
    with open(os.path.join(HERE, "og.html"), "w", encoding="utf-8", newline="\n") as f:
        f.write(card)

if __name__ == "__main__":
    import pages
    pages.build(sys.modules[__name__])
    ddir = os.path.join(HERE, "docs")
    for _, items in DOCS:
        for f, t, _ in items:
            m, body = read_fragment(os.path.join(ddir, f))
            docs_page(f, m["title"], m["desc"], body)
    write_og_card()
    print("built")


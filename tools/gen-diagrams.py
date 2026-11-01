import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch, FancyArrowPatch
import os

OUT = os.path.expanduser("~/scratch/cliptrail/docs/images")
os.makedirs(OUT, exist_ok=True)

C = {
    "device": ("#E8F0FE", "#1A73E8", "#174EA6"),
    "relay":  ("#FCE8E6", "#D93025", "#A50E0E"),
    "lan":    ("#E6F4EA", "#188038", "#0D652D"),
    "web":    ("#FEF7E0", "#F29900", "#B06000"),
    "core":   ("#F3E8FD", "#9334E6", "#681DA8"),
    "neutral":("#F1F3F4", "#5F6368", "#3C4043"),
    "drop":   ("#FCE8E6", "#D93025", "#A50E0E"),
    "ok":     ("#E6F4EA", "#188038", "#0D652D"),
}

def box(ax, x, y, w, h, title, sub=None, kind="device", fs=11):
    fc, ec, tc = C[kind]
    ax.add_patch(FancyBboxPatch((x, y), w, h, boxstyle="round,pad=0.4,rounding_size=1.2",
                              fc=fc, ec=ec, lw=1.6, zorder=2))
    cy = y + h / 2 + (1.2 if sub else 0)
    ax.text(x + w / 2, cy, title, ha="center", va="center", fontsize=fs,
            fontweight="bold", color=tc, zorder=3)
    if sub:
        ax.text(x + w / 2, y + h / 2 - 2.2, sub, ha="center", va="center",
                fontsize=fs - 2, color=tc, zorder=3)

def arrow(ax, p1, p2, label=None, color="#5F6368", style="arc3,rad=0", lw=1.8, fs=8.5, lab_off=(0, 0)):
    ax.add_patch(FancyArrowPatch(p1, p2, arrowstyle="-|>", mutation_scale=16,
                                 connectionstyle=style, color=color, lw=lw, zorder=1))
    if label:
        mx, my = (p1[0] + p2[0]) / 2 + lab_off[0], (p1[1] + p2[1]) / 2 + lab_off[1]
        ax.text(mx, my, label, ha="center", va="center", fontsize=fs, color=color,
                fontweight="bold", zorder=4,
                bbox=dict(fc="white", ec="none", alpha=0.9, pad=1.5))

def canvas(w, h, title):
    fig, ax = plt.subplots(figsize=(w, h), dpi=150)
    ax.set_xlim(0, 100); ax.set_ylim(0, 60); ax.axis("off")
    ax.text(50, 57.5, title, ha="center", va="center", fontsize=16, fontweight="bold", color="#202124")
    return fig, ax

def save(fig, name):
    fig.savefig(f"{OUT}/{name}", bbox_inches="tight", facecolor="white")
    plt.close(fig)
    print("wrote", name)

# ---------- 1. Big picture ----------
fig, ax = canvas(12.5, 7.6, "ClipTrail — System Overview")

box(ax, 4, 40, 26, 12, "Desktop app", "Win · macOS · Linux\nTauri 2 + clipd-core\ncapture · picker · dashboard")
box(ax, 38, 40, 24, 12, "Android app", "Kotlin shell + UniFFI\nIME keyboard · tile · share")
box(ax, 70, 40, 26, 12, "iOS (later)", "share ext · shortcut\nkeyboard · read-only")

box(ax, 12, 18, 30, 10, "LAN / P2P", "mDNS discover · direct sync\n(both devices online)", "lan")
box(ax, 58, 18, 30, 10, "Relay (axum)", "WS + REST · stores ciphertext only\nhosted OR one docker run", "relay")
box(ax, 63, 2, 24, 8, "Web dashboard", "read-only · same UI", "web")

arrow(ax, (17, 40), (17, 28.5), "direct when online", "#188038", fs=9)
arrow(ax, (17, 28.5), (17, 40), None, "#188038")
arrow(ax, (50, 40), (50, 33), "catch-up", "#D93025", fs=9)
arrow(ax, (50, 33), (50, 40), None, "#D93025")
arrow(ax, (73, 40), (73, 28.5), "remote / offline", "#D93025", fs=9)
arrow(ax, (73, 28.5), (73, 40), None, "#D93025")
arrow(ax, (75, 18), (75, 10.5), "WS/REST", "#F29900", fs=9)
arrow(ax, (75, 10.5), (75, 18), None, "#F29900")

ax.text(36, 13.5, "sealed envelopes — relay stores & forwards\nbut cannot read clip contents",
        ha="center", fontsize=9.5, style="italic", color="#A50E0E")
save(fig, "architecture.png")

# ---------- 2. Capture pipeline ----------
fig, ax = canvas(13.5, 5.2, "Capture pipeline — every OS clipboard write")

steps = [
    ("Watcher", "OS clipboard\npoll/hook", "neutral"),
    ("Normalize", "text · image\nfile-ref", "neutral"),
    ("Hash", "SHA-256", "neutral"),
    ("Dedup?", "same hash?", "web"),
    ("Classify", "rules → heuristics\n→ magika*", "core"),
    ("Metadata", "app · domain\nlang · thumb", "core"),
    ("Rules", "ignore · sensitive\npin · section", "core"),
    ("Persist", "SQLite\n+ FTS5", "device"),
    ("Notify", "UI + sync\nevent", "lan"),
]
x = 1; W = 9.8; gap = 1.4; Y = 34; H = 9
for i, (t, s, k) in enumerate(steps):
    box(ax, x, Y, W, H, t, s, k, fs=10)
    if i < len(steps) - 1:
        arrow(ax, (x + W, Y + H / 2), (x + W + gap, Y + H / 2), None, "#5F6368")
    x += W + gap

dx = 1 + (W + gap) * 3 + W / 2
box(ax, dx - 4, 14, 10, 7, "Drop", None, "drop", fs=10)
arrow(ax, (dx, Y), (dx, 21.5), "dup", "#D93025", fs=8)

rx = 1 + (W + gap) * 6 + W / 2
arrow(ax, (rx, Y), (rx, 21.5), "ignore/sensitive", "#D93025", fs=8, lab_off=(6, -3))

ax.text(50, 6, "watcher sees the system clipboard — terminal, Chrome, PDFs, IDE, anything · "
               "concealed/password-manager clips skipped by design",
        ha="center", fontsize=9.5, style="italic", color="#3C4043")
save(fig, "pipeline.png")

# ---------- 3. Sync ----------
fig, ax = canvas(12.5, 6.6, "Sync — sealed envelopes, relay can't read")

box(ax, 6, 34, 22, 13, "Laptop", "encrypt clip\nXChaCha20 · MasterKey")
box(ax, 39, 34, 22, 13, "Relay", "ciphertext mailbox\nhosted or self-hosted")
box(ax, 72, 34, 22, 13, "Phone", "decrypt → dedup\n→ FTS index")

arrow(ax, (28, 44), (39, 44), "POST /events", "#D93025", fs=9)
arrow(ax, (39, 41.5), (28, 41.5), None, "#D93025")
arrow(ax, (61, 44), (72, 44), "WS push / since=seq", "#D93025", fs=9)
arrow(ax, (72, 41.5), (61, 41.5), None, "#D93025")

ax.add_patch(FancyArrowPatch((17, 34), (83, 34), arrowstyle="<|-|>", mutation_scale=16,
                             connectionstyle="arc3,rad=0.35", color="#188038", lw=2, zorder=1))
ax.text(50, 16, "both online → direct P2P / LAN — relay not needed", ha="center",
        fontsize=10, fontweight="bold", color="#188038",
        bbox=dict(fc="white", ec="none", alpha=0.9, pad=2))

ax.text(50, 5, "events are append-only: ClipAdded · ClipUpdated · ClipDeleted(tombstone) · DevicePaired/Revoked",
        ha="center", fontsize=9.5, style="italic", color="#3C4043")
save(fig, "sync.png")
print("done")

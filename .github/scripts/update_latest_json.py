"""Point latest.json at re-signed installers.

Usage: update_latest_json.py <latest.json in> <signed dir> <latest.json out>

Code signing changes the Windows installers after tauri-action wrote
latest.json, so the updater signatures in it no longer match. For every
platform entry whose download file name has a fresh `<file>.sig` under the
signed dir, replace its signature. Fails if no entry was updated.
"""

import json
import pathlib
import sys
from urllib.parse import unquote, urlparse


def main() -> None:
    src, signed_dir, out = sys.argv[1:4]
    manifest = json.loads(pathlib.Path(src).read_text(encoding="utf-8"))
    sigs = {p.name[: -len(".sig")]: p.read_text(encoding="utf-8").strip()
            for p in pathlib.Path(signed_dir).rglob("*.sig")}
    updated = []
    for name, entry in manifest.get("platforms", {}).items():
        file = unquote(pathlib.PurePosixPath(urlparse(entry["url"]).path).name)
        if file in sigs:
            entry["signature"] = sigs[file]
            updated.append(f"{name} -> {file}")
    if not updated:
        sys.exit(f"no latest.json entry matched the signed files: {sorted(sigs)}")
    pathlib.Path(out).write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print("updated:\n  " + "\n  ".join(updated))


if __name__ == "__main__":
    main()

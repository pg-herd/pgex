"""Pin each supported Nix platform to the published release archive checksum."""

import json
import re
import sys
import urllib.request
from pathlib import Path


if __name__ == "__main__":
    tag, output = sys.argv[1:]
    if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", tag):
        raise ValueError("Expected a stable release tag")
    targets = {
        "x86_64-linux": "x86_64-unknown-linux-gnu",
        "aarch64-linux": "aarch64-unknown-linux-gnu",
        "aarch64-darwin": "aarch64-apple-darwin",
    }
    platforms = {}
    for system, target in targets.items():
        archive = f"pgex-{tag}-{target}.tar.gz"
        url = f"https://github.com/pg-herd/pgex/releases/download/{tag}/{archive}.sha256"
        with urllib.request.urlopen(url, timeout=60) as response:
            checksum, filename = response.read().decode().strip().split()
        if filename.lstrip("*") != archive or not re.fullmatch(r"[0-9a-f]{64}", checksum):
            raise ValueError(f"Invalid checksum file for {archive}")
        platforms[system] = {"target": target, "sha256": checksum}
    Path(output).write_text(json.dumps({"version": tag[1:], "platforms": platforms}, indent=2) + "\n")

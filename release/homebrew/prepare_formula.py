"""Point the formula at a verified release before Homebrew builds its bottle."""

import re
import sys
from pathlib import Path


def prepare_formula(formula: str, tag: str, checksum: str) -> str:
    if not re.fullmatch(r"v\d+\.\d+\.\d+", tag):
        raise ValueError("Expected a stable release tag such as v0.2.0")
    if not re.fullmatch(r"[0-9a-f]{64}", checksum):
        raise ValueError("Expected the verified archive's SHA-256")
    replacements = {
        "url": f"https://github.com/pg-herd/pgex/releases/download/{tag}/pgex-{tag}-aarch64-apple-darwin.tar.gz",
        "version": tag[1:],
        "sha256": checksum,
    }
    for field, value in replacements.items():
        formula, count = re.subn(
            rf'^  {field} "[^"\n]*"$', f'  {field} "{value}"', formula, flags=re.MULTILINE
        )
        if count != 1:
            raise ValueError(f"Expected one top-level {field} in the formula")
    # Homebrew writes the new block from its own JSON metadata after publication.
    return re.sub(r"\n  bottle do\n.*?\n  end\n", "", formula, flags=re.DOTALL)


if __name__ == "__main__":
    path, tag, checksum = sys.argv[1:]
    path = Path(path)
    path.write_text(prepare_formula(path.read_text(), tag, checksum))

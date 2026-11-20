"""Generate the Arch recipe from a verified release archive's checksum."""

import re
import sys
from pathlib import Path


if __name__ == "__main__":
    template, name, tag, checksum, output = sys.argv[1:]
    if name not in {"pgex", "pgex-bin"}:
        raise ValueError("Expected pgex for pacman or pgex-bin for AUR")
    if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", tag):
        raise ValueError("Expected a stable release tag")
    if not re.fullmatch(r"[0-9a-f]{64}", checksum):
        raise ValueError("Expected the verified archive's SHA-256")
    recipe = Path(template).read_text()
    for marker, value in (("@NAME@", name), ("@VERSION@", tag[1:]), ("@SHA256@", checksum)):
        if recipe.count(marker) != 1:
            raise ValueError(f"Expected one {marker} in the recipe")
        recipe = recipe.replace(marker, value)
    Path(output).write_text(recipe)

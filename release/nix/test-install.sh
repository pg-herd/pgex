#!/usr/bin/env bash
set -euo pipefail

[[ $# == 1 ]] || { echo "Usage: $0 REPOSITORY" >&2; exit 2; }
repository=$(cd "$1" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
export NIX_CONFIG="${NIX_CONFIG:-}"$'\nexperimental-features = nix-command flakes'
mkdir -p "$work/flake/release/nix"
cp "$repository/flake.nix" "$repository/flake.lock" "$work/flake/"
cp "$repository/release/nix/"{package.nix,sources.json} "$work/flake/release/nix/"
version=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$work/flake/release/nix/sources.json")

# Use a real earlier release, with the same flake URL, to exercise profile upgrade.
python3 "$repository/release/nix/update-release.py" v0.1.4 "$work/flake/release/nix/sources.json"
nix profile install --profile "$work/profile" "path:$work/flake#pgex"
before=$(python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$work/profile/bin/pgex")
[[ "$before" == *-pgex-0.1.4/bin/pgex ]]
LD_BIND_NOW=1 "$work/profile/bin/pgex" --help >/dev/null

cp "$repository/release/nix/sources.json" "$work/flake/release/nix/sources.json"
nix profile upgrade --refresh --profile "$work/profile" pgex
after=$(python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$work/profile/bin/pgex")
[[ "$after" == *-pgex-$version/bin/pgex && "$after" != "$before" ]]
LD_BIND_NOW=1 "$work/profile/bin/pgex" --help >/dev/null
nix run "path:$work/flake" -- --help >/dev/null
nix profile remove --profile "$work/profile" pgex
[[ ! -e "$work/profile/bin/pgex" ]]
printf 'PASS: Nix profile installation, upgrade from 0.1.4, nix run, and removal\n'

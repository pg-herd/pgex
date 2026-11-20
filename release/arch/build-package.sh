#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 3 || $(id -u) != 0 || ! -f /.dockerenv ]]; then
  echo "Run in a disposable Arch container: $0 PKGBUILD RELEASE_ARCHIVE OUTPUT_DIRECTORY" >&2
  exit 2
fi
recipe=$(realpath "$1")
archive=$(realpath "$2")
mkdir -p "$3"
output=$(realpath "$3")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
chmod 755 "$work"
cp "$recipe" "$work/PKGBUILD"
cp "$archive" "$work/"
chown -R builder:builder "$work"
cd "$work"
runuser -u builder -- makepkg --cleanbuild --nodeps
packages=("$work"/*.pkg.tar.zst)
[[ ${#packages[@]} == 1 && -f "${packages[0]}" ]]
tar -xOf "${packages[0]}" usr/bin/pgex > "$work/pgex"
tar -xOf "$archive" --wildcards '*/pgex' | cmp - "$work/pgex"
runuser -u builder -- makepkg --printsrcinfo > "$output/.SRCINFO"
cp -- "$recipe" "$output/PKGBUILD"
cp -- "${packages[0]}" "$output/"

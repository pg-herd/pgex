#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 3 ]]; then
  echo "Usage: $0 PACKAGE_DIRECTORY OUTPUT_DIRECTORY SIGNING_KEY_FINGERPRINT" >&2
  exit 2
fi
packages=$(realpath "$1")
output=$2
key=$3
[[ "$key" =~ ^[[:xdigit:]]{40}$ ]]
mkdir -p "$output/x86_64"
shopt -s nullglob
files=("$packages"/*.pkg.tar.zst)
[[ ${#files[@]} -gt 0 ]]

sign() {
  gpg --batch --yes --pinentry-mode loopback --passphrase-fd 3 --local-user "$key" \
    --detach-sign --output "$1.sig" "$1" 3<<<"${APT_SIGNING_PASSPHRASE:-}"
}

gpg --batch --export-options export-minimal --armor --export "$key" > "$output/pgex.asc"
cd "$output/x86_64"
for package in "${files[@]}"; do
  filename=${package##*/}
  [[ "$filename" =~ ^pgex-[0-9]+\.[0-9]+\.[0-9]+-[0-9]+-x86_64\.pkg\.tar\.zst$ ]]
  install -m644 "$package" "$filename"
  sign "$filename"
done
repo-add --prevent-downgrade pgex.db.tar.gz ./*.pkg.tar.zst
# GitHub Pages artifacts must use regular files instead of repo-add's symlinks.
rm -- pgex.db pgex.files
mv -- pgex.db.tar.gz pgex.db
mv -- pgex.files.tar.gz pgex.files
sign pgex.db
sign pgex.files

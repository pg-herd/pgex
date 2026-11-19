#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 3 ]]; then
  echo "Usage: $0 PACKAGE_DIRECTORY OUTPUT_DIRECTORY PRIVATE_SIGNING_KEY" >&2
  exit 2
fi
packages=$(realpath "$1")
mkdir -p "$2"
output=$(realpath "$2")
key=$(realpath "$3")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir "$work/keys"
openssl rsa -in "$key" -pubout -out "$work/keys/pgex.rsa.pub" 2>/dev/null
install -m644 "$work/keys/pgex.rsa.pub" "$output/pgex.rsa.pub"
shopt -s nullglob
files=("$packages"/*.apk)
[[ ${#files[@]} -gt 0 ]]
for package in "${files[@]}"; do
  apk verify --keys-dir "$work/keys" "$package"
  metadata=$(tar --ignore-zeros -xzOf "$package" .PKGINFO)
  name=$(awk -F ' = ' '$1 == "pkgname" {print $2}' <<<"$metadata")
  version=$(awk -F ' = ' '$1 == "pkgver" {print $2}' <<<"$metadata")
  architecture=$(awk -F ' = ' '$1 == "arch" {print $2}' <<<"$metadata")
  [[ "$name" == pgex && "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(_alpha[0-9]+)?-r[0-9]+$ ]]
  case "$architecture" in x86_64|aarch64) ;; *) exit 2 ;; esac
  mkdir -p "$output/$architecture"
  cp "$package" "$output/$architecture/pgex-$version.apk"
done
for architecture in x86_64 aarch64; do
  [[ -d "$output/$architecture" ]] || continue
  (
    cd "$output/$architecture"
    apk index --keys-dir "$work/keys" --description 'pgex packages' --output APKINDEX.tar.gz ./*.apk
    abuild-sign -k "$key" -p "$output/pgex.rsa.pub" APKINDEX.tar.gz
    apk verify --keys-dir "$work/keys" APKINDEX.tar.gz
  )
done

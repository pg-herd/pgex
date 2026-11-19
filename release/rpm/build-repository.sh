#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 || $# -gt 4 ]]; then
  echo "Usage: $0 SIGNED_PACKAGE_DIRECTORY OUTPUT_DIRECTORY SIGNING_KEY_FINGERPRINT [REPOSITORY_URL]" >&2
  exit 2
fi
packages=$(realpath "$1")
mkdir -p "$2"
output=$(realpath "$2")
key=$3
repository_url=${4:-https://pg-herd.github.io/pgex/rpm}
[[ "$key" =~ ^[[:xdigit:]]{40}$ || "$key" =~ ^[[:xdigit:]]{64}$ ]]
gpg --batch --export-options export-minimal --armor --export "$key" > "$output/pgex.asc"
verification=$(mktemp -d)
trap 'rm -rf "$verification"' EXIT
rpm --dbpath "$verification" --initdb
rpm --dbpath "$verification" --import "$output/pgex.asc"
shopt -s nullglob
files=("$packages"/*.rpm)
[[ ${#files[@]} -gt 0 ]]
for package in "${files[@]}"; do
  [[ "$(rpm -qp --qf '%{NAME}' "$package")" == pgex ]]
  architecture=$(rpm -qp --qf '%{ARCH}' "$package")
  case "$architecture" in x86_64|aarch64) ;; *) exit 2 ;; esac
  signature=$(rpmkeys --dbpath "$verification" --checksig --verbose "$package")
  printf '%s\n' "$signature"
  grep -E 'Signature.*: OK' <<<"$signature"
  mkdir -p "$output/$architecture/Packages"
  cp "$package" "$output/$architecture/Packages/"
done
for architecture in x86_64 aarch64; do
  [[ -d "$output/$architecture/Packages" ]] || continue
  createrepo_c --simple-md-filenames "$output/$architecture"
  metadata="$output/$architecture/repodata/repomd.xml"
  gpg --batch --yes --pinentry-mode loopback --passphrase-fd 3 --local-user "$key" \
    --armor --detach-sign --output "$metadata.asc" "$metadata" 3<<<"${APT_SIGNING_PASSPHRASE:-}"
done
cat > "$output/pgex.repo" <<REPOSITORY
[pgex]
name=pgex
baseurl=$repository_url/\$basearch
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=$repository_url/pgex.asc
sslverify=1
skip_if_unavailable=0
REPOSITORY

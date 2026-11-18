#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 || $# -gt 4 ]]; then
  echo "Usage: $0 PACKAGE_DIRECTORY OUTPUT_DIRECTORY SIGNING_KEY_FINGERPRINT [REPOSITORY_URL]" >&2
  exit 2
fi
packages=$(realpath "$1")
mkdir -p "$2"
output=$(realpath "$2")
key=$3
repository_url=${4:-https://pg-herd.github.io/pgex/apt}
[[ "$key" =~ ^[[:xdigit:]]{40}$ || "$key" =~ ^[[:xdigit:]]{64}$ ]]
shopt -s nullglob
files=("$packages"/*.deb)
[[ ${#files[@]} -gt 0 ]]
mkdir -p "$output/pool/main/p/pgex"
for package in "${files[@]}"; do
  [[ "$(dpkg-deb -f "$package" Package)" == pgex ]]
  case "$(dpkg-deb -f "$package" Architecture)" in amd64|arm64) ;; *) exit 2 ;; esac
  cp "$package" "$output/pool/main/p/pgex/"
done

for architecture in amd64 arm64; do
  index="$output/dists/stable/main/binary-$architecture"
  mkdir -p "$index"
  (cd "$output" && dpkg-scanpackages --arch "$architecture" --multiversion pool /dev/null) > "$index/Packages"
  gzip -n -9 -c "$index/Packages" > "$index/Packages.gz"
done

config=$(mktemp)
trap 'rm -f "$config"' EXIT
cat > "$config" <<'CONFIG'
APT::FTPArchive::Release {
  Origin "pgex";
  Label "pgex";
  Suite "stable";
  Codename "stable";
  Architectures "amd64 arm64";
  Components "main";
  Description "Official pgex packages";
};
CONFIG
release="$output/dists/stable"
rm -f "$release/Release" "$release/InRelease" "$release/Release.gpg"
(cd "$output" && apt-ftparchive -c "$config" release dists/stable) > "$release/Release"

# The passphrase travels over a file descriptor, never a command-line argument.
gpg --batch --yes --pinentry-mode loopback --passphrase-fd 3 --local-user "$key" \
  --clearsign --output "$release/InRelease" "$release/Release" 3<<<"${APT_SIGNING_PASSPHRASE:-}"
gpg --batch --yes --pinentry-mode loopback --passphrase-fd 3 --local-user "$key" \
  --armor --detach-sign --output "$release/Release.gpg" "$release/Release" 3<<<"${APT_SIGNING_PASSPHRASE:-}"
gpg --batch --export-options export-minimal --armor --export "$key" > "$output/pgex.asc"
cat > "$output/pgex.sources" <<SOURCE
Types: deb
URIs: $repository_url
Suites: stable
Components: main
Signed-By: /etc/apt/keyrings/pgex.asc
SOURCE
touch "$output/.nojekyll"

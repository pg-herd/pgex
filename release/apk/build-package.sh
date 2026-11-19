#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 4 ]]; then
  echo "Usage: $0 EXTRACTED_RELEASE VERSION ARCHITECTURE OUTPUT_DIRECTORY" >&2
  exit 2
fi
export RELEASE_INPUT=$(realpath "$1")
export PACKAGE_VERSION=$2
architecture=$3
mkdir -p "$4"
output=$(realpath "$4")
[[ "$PACKAGE_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(_alpha[0-9]+)?$ ]]
case "$architecture" in x86_64|aarch64) ;; *) echo "Unsupported architecture: $architecture" >&2; exit 2 ;; esac
[[ "$(apk --print-arch)" == "$architecture" ]]
[[ -x "$RELEASE_INPUT/pgex" && -f "$RELEASE_INPUT/LICENSE" && -d "$RELEASE_INPUT/licenses" ]]
[[ -f "${PACKAGER_PRIVKEY:?Set PACKAGER_PRIVKEY}" ]]
recipe=$(realpath "$(dirname "$0")/APKBUILD")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/aports/pgex"
cp "$recipe" "$work/aports/pgex/APKBUILD"
export PACKAGER_PUBKEY="$work/pgex.rsa.pub"
openssl rsa -in "$PACKAGER_PRIVKEY" -pubout -out "$PACKAGER_PUBKEY" 2>/dev/null
cp "$PACKAGER_PUBKEY" /etc/apk/keys/pgex.rsa.pub
(cd "$work/aports/pgex" && abuild -F -d -P "$work/packages")
package="$work/packages/aports/$architecture/pgex-$PACKAGE_VERSION-r0.apk"
apk verify "$package"
# Preserve the executable, including its release optimizations and debug stripping.
tar --ignore-zeros -xzOf "$package" usr/bin/pgex > "$work/pgex"
cmp "$RELEASE_INPUT/pgex" "$work/pgex"
# Release asset names include the architecture; repository names are canonical.
cp "$package" "$output/pgex-$PACKAGE_VERSION-r0.$architecture.apk"

#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 3 || ! -f /.dockerenv || $(id -u) != 0 ]]; then
  echo "Run in a disposable Arch container: $0 BASE_URL VERSION INSTALLER" >&2
  exit 2
fi
base=$1
version=$2
installer=$3
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Docker Desktop's x86-64 emulation cannot apply pacman's seccomp filters.
if [[ ${PACMAN_FLAGS:-} == --disable-sandbox ]]; then
  printf '#!/bin/sh\nexec /usr/bin/pacman --disable-sandbox "$@"\n' > "$work/pacman"
  chmod +x "$work/pacman"
  export PATH="$work:$PATH"
fi

sed 's/^  fingerprint=.*/  fingerprint=0000000000000000000000000000000000000000/' "$installer" > "$work/wrong-key.sh"
if sh "$work/wrong-key.sh" > "$work/wrong-key.log" 2>&1; then
  echo 'Installer accepted an unexpected signing key.' >&2
  exit 1
fi
grep -F 'signing key does not match the pgex fingerprint' "$work/wrong-key.log"
[[ ! -e /etc/pacman.d/pgex.conf ]]

cat "$installer" | sh
[[ $(pacman -Q pgex) == "pgex $version-1" ]]
LD_BIND_NOW=1 pgex --help >/dev/null
pacman -Qk pgex

cat "$installer" | sh
[[ $(grep -Fxc 'Include = /etc/pacman.d/pgex.conf' /etc/pacman.conf) == 1 ]]

pacman -R --noconfirm pgex
sed -i "s|$base/arch/|$base/old/arch/|" /etc/pacman.d/pgex.conf
rm -f /var/lib/pacman/sync/pgex.db{,.sig}
pacman -Syu --noconfirm pgex
[[ $(pacman -Q pgex) == "pgex $version-0" ]]
sed -i "s|$base/old/arch/|$base/arch/|" /etc/pacman.d/pgex.conf
rm -f /var/lib/pacman/sync/pgex.db{,.sig}
pacman -Syu --noconfirm
[[ $(pacman -Q pgex) == "pgex $version-1" ]]
LD_BIND_NOW=1 pgex --help >/dev/null

signature=/var/lib/pacman/sync/pgex.db.sig
cp "$signature" "$work/pgex.db.sig"
curl -fsSL "$base/old/arch/$(uname -m)/pgex.db.sig" -o "$signature"
if pacman -Sl pgex > "$work/bad-signature.log" 2>&1; then
  echo 'Pacman accepted an index with an invalid signature.' >&2
  exit 1
fi
grep -F 'invalid or corrupted database' "$work/bad-signature.log"
cp "$work/pgex.db.sig" "$signature"

pacman -R --noconfirm pgex
[[ ! -e /usr/bin/pgex ]]
echo 'PASS: Arch installer, pinned signing key, index signatures, repeat setup, upgrade, and removal'

#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 2 ]]; then
  echo "Usage: $0 PACKAGE_DIRECTORY SIGNING_KEY_FINGERPRINT" >&2
  exit 2
fi
directory=$(realpath "$1")
key=$2
[[ "$key" =~ ^[[:xdigit:]]{40}$ || "$key" =~ ^[[:xdigit:]]{64}$ ]]
[[ -d "${GNUPGHOME:?Set GNUPGHOME}" ]]
shopt -s nullglob
packages=("$directory"/*.rpm)
[[ ${#packages[@]} -gt 0 ]]
passphrase=$(mktemp)
trap 'rm -f "$passphrase"' EXIT
chmod 600 "$passphrase"
printf '%s' "${APT_SIGNING_PASSPHRASE:-}" > "$passphrase"
rpmsign --define "_gpg_name $key" --define "_gpg_path $GNUPGHOME" \
  --define "__gpg $(command -v gpg)" \
  --define "_gpg_sign_cmd_extra_args --batch --pinentry-mode loopback --passphrase-file $passphrase" \
  --addsign "${packages[@]}"

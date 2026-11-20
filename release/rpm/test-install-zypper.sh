#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 4 || ! -f /.dockerenv || $(id -u) != 0 ]]; then
  echo "Run in a disposable openSUSE container: $0 REPOSITORY_URL VERSION PREVIOUS_PACKAGE INSTALLER" >&2
  exit 2
fi
repository=$1
version=$2
previous=$3
installer=$4
config=/etc/zypp/repos.d/pgex.repo
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

if ! command -v curl >/dev/null; then
  zypper --non-interactive install --no-recommends curl
fi
cat "$installer" | sh
grep -Fx "baseurl=$repository/\$basearch" "$config"
grep -Fx 'autorefresh=1' "$config"
grep -Fx 'repo_gpgcheck=1' "$config"
grep -Fx 'pkg_gpgcheck=1' "$config"
[[ $(rpm -q --qf '%{VERSION}-%{RELEASE}' pgex) == "$version-1" ]]
LD_BIND_NOW=1 pgex --help >/dev/null
rpm --verify pgex

zypper --non-interactive remove pgex
zypper --non-interactive install "$previous"
[[ $(rpm -q --qf '%{VERSION}-%{RELEASE}' pgex) == "$version-0" ]]
zypper --non-interactive update pgex
[[ $(rpm -q --qf '%{VERSION}-%{RELEASE}' pgex) == "$version-1" ]]
LD_BIND_NOW=1 pgex --help >/dev/null
zypper --non-interactive remove pgex
[[ ! -e /usr/bin/pgex ]]

sed -i "s|^baseurl=$repository/|baseurl=$repository-invalid/|" "$config"
if zypper --non-interactive refresh --force pgex > "$work/invalid-signature" 2>&1; then
  echo 'Zypper accepted an invalid repository signature.' >&2
  exit 1
fi
grep -Ei 'verification failed|invalid signature|not signed|signature check' "$work/invalid-signature"
printf 'PASS: Zypper installation, upgrade, removal, and invalid metadata signature rejection\n'

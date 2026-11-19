#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 5 ]]; then
  echo "Usage: $0 REPOSITORY_URL PUBLIC_KEY EXPECTED_VERSION PREVIOUS_PACKAGE INSTALLER" >&2
  exit 2
fi
repository=$1
key=$2
version=$3
previous=$4
installer=$5
dnf install -y ca-certificates curl-minimal
sh "$installer"
grep -Fx "baseurl=$repository/\$basearch" /etc/yum.repos.d/pgex.repo
grep -Fx 'gpgcheck=1' /etc/yum.repos.d/pgex.repo
grep -Fx 'repo_gpgcheck=1' /etc/yum.repos.d/pgex.repo
[[ "$(rpm -q --qf '%{VERSION}-%{RELEASE}' pgex)" == "$version-1" ]]
LD_BIND_NOW=1 pgex --help >/dev/null
rpm --verify pgex
[[ -f /usr/share/licenses/pgex/LICENSE && -d /usr/share/licenses/pgex/third-party ]]
for compiler in cargo rustc clang; do
  if command -v "$compiler"; then
    echo "Unexpected compiler installed: $compiler" >&2
    exit 1
  fi
done
set +e
pgex --source postgresql://localhost/source --target postgresql://localhost/target \
  --where 'id =' > /tmp/pgex-output 2>&1
status=$?
set -e
[[ $status == 2 ]]
grep -F 'Invalid --where predicate' /tmp/pgex-output
grep -F 'Line 1, column 5' /tmp/pgex-output

dnf remove -y pgex
[[ ! -e /usr/bin/pgex ]]
dnf install -y --setopt=localpkg_gpgcheck=1 "$previous"
[[ "$(rpm -q --qf '%{VERSION}-%{RELEASE}' pgex)" == "$version-0" ]]
dnf upgrade -y pgex
[[ "$(rpm -q --qf '%{VERSION}-%{RELEASE}' pgex)" == "$version-1" ]]
LD_BIND_NOW=1 pgex --help >/dev/null
dnf remove -y pgex
[[ ! -e /usr/bin/pgex ]]

# Verify the actual package signature, including rejection without a trusted key.
verification=$(mktemp -d)
trap 'rm -rf "$verification"' EXIT
rpm --dbpath "$verification" --initdb
if rpmkeys --dbpath "$verification" --checksig "$previous" > /tmp/pgex-untrusted-package 2>&1; then
  echo 'RPM accepted a package without its trusted signing key' >&2
  exit 1
fi
grep -Ei 'NOKEY|NOT OK|signature' /tmp/pgex-untrusted-package
rpm --dbpath "$verification" --import "$key"
rpmkeys --dbpath "$verification" --checksig "$previous"

# A different valid key must not authenticate repository metadata. The tracked
# production public key differs from the temporary key used by these tests.
sed -i 's|^gpgkey=.*|gpgkey=file:///work/release/pgex.asc|' /etc/yum.repos.d/pgex.repo
for cache in /var/cache/dnf/pgex-* /var/cache/libdnf5/pgex-*; do
  [[ ! -d "$cache" ]] || rm -rf "$cache"
done
if dnf -y --repo=pgex --setopt=retries=0 makecache > /tmp/pgex-untrusted-metadata 2>&1; then
  echo 'DNF accepted repository metadata without its trusted signing key' >&2
  exit 1
fi
grep -Ei 'GPG|signature|verification|key' /tmp/pgex-untrusted-metadata
printf 'PASS: generic installer, installation, dependency resolution, SQL diagnostics, upgrade, removal, package and metadata signature rejection\n'

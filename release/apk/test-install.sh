#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 5 ]]; then
  echo "Usage: $0 REPOSITORY_URL PUBLIC_KEY EXPECTED_VERSION PREVIOUS_REPOSITORY_URL INSTALLER" >&2
  exit 2
fi
repository=$1
key=$2
version=$3
previous=$4
installer=$5
apk add --no-cache ca-certificates curl
sh "$installer"
cmp "$key" /etc/apk/keys/pgex.rsa.pub
grep -Fx "$repository" /etc/apk/repositories
sh "$installer"
[[ "$(grep -Fxc "$repository" /etc/apk/repositories)" == 1 ]]
apk info --exists "pgex=$version-r0"
LD_BIND_NOW=1 pgex --help >/dev/null
[[ -f /usr/share/doc/pgex/LICENSE && -d /usr/share/doc/pgex/licenses ]]
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

apk del pgex
[[ ! -e /usr/bin/pgex ]]
# Install the older version from its own signed repository, as a user would.
# Installing a local APK instead would pin its digest in /etc/apk/world.
cp /etc/apk/repositories /tmp/pgex-repositories
grep -Fxv "$repository" /tmp/pgex-repositories > /etc/apk/repositories
printf '%s\n' "$previous" >> /etc/apk/repositories
apk add --update-cache pgex
apk info --exists "pgex=${version}_alpha1-r0"
cp /tmp/pgex-repositories /etc/apk/repositories
apk upgrade --update-cache pgex
apk info --exists "pgex=$version-r0"
LD_BIND_NOW=1 pgex --help >/dev/null
apk del pgex
[[ ! -e /usr/bin/pgex ]]
# Both the repository index and package must fail without the trusted key.
rm /etc/apk/keys/pgex.rsa.pub
if apk --no-cache update > /tmp/pgex-untrusted-index 2>&1; then
  echo 'APK accepted the repository without its trusted signing key' >&2
  exit 1
fi
grep -Ei 'untrusted|signature' /tmp/pgex-untrusted-index
if apk add "$repository/$(apk --print-arch)/pgex-$version-r0.apk" > /tmp/pgex-untrusted-package 2>&1; then
  echo 'APK accepted a package without its trusted signing key' >&2
  exit 1
fi
grep -Ei 'untrusted|signature' /tmp/pgex-untrusted-package
printf 'PASS: generic installer, dependencies, SQL diagnostics, upgrade, removal, and signature rejection\n'

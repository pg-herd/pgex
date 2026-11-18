#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 4 ]]; then
  echo "Usage: $0 REPOSITORY_URL PUBLIC_KEY EXPECTED_VERSION PREVIOUS_PACKAGE" >&2
  exit 2
fi
repository=$1
key=$2
version=$3
previous=$4
export DEBIAN_FRONTEND=noninteractive
# Match the README bootstrap: HTTPS repositories need the CA trust store first.
apt-get update
apt-get install -y --no-install-recommends ca-certificates
install -Dm644 "$key" /etc/apt/keyrings/pgex.asc
sed "s|^URIs:.*|URIs: $repository|" "$(dirname "$key")/pgex.sources" \
  > /etc/apt/sources.list.d/pgex.sources
apt-get update
apt-get install -y --no-install-recommends pgex
[[ "$(dpkg-query -W -f='${Version}' pgex)" == "$version" ]]
LD_BIND_NOW=1 pgex --help >/dev/null
[[ -f /usr/share/doc/pgex/copyright && -d /usr/share/doc/pgex/licenses ]]
for compiler in cargo rustc clang; do
  if command -v "$compiler"; then
    echo "Unexpected compiler installed: $compiler" >&2
    exit 1
  fi
done

# Check the offline parser in the installed binary, before any DB connection.
set +e
pgex --source postgresql://localhost/source --target postgresql://localhost/target \
  --where 'id =' > /tmp/pgex-output 2>&1
status=$?
set -e
[[ $status == 2 ]]
grep -F 'Invalid --where predicate' /tmp/pgex-output
grep -F 'Line 1, column 5' /tmp/pgex-output

apt-get remove -y pgex
[[ ! -e /usr/bin/pgex ]]
apt-get install -y --no-install-recommends "$previous"
[[ "$(dpkg-query -W -f='${Version}' pgex)" != "$version" ]]
apt-get install -y --only-upgrade pgex
[[ "$(dpkg-query -W -f='${Version}' pgex)" == "$version" ]]
LD_BIND_NOW=1 pgex --help >/dev/null
apt-get purge -y pgex
[[ ! -e /usr/bin/pgex ]]
# A repository whose signing key is not trusted must be rejected.
: > /etc/apt/keyrings/pgex.asc
if apt-get -o APT::Update::Error-Mode=any update > /tmp/pgex-untrusted 2>&1; then
  echo "APT accepted the repository without its trusted signing key" >&2
  exit 1
fi
grep -E 'NO_PUBKEY|signature' /tmp/pgex-untrusted
printf 'PASS: installation, dependency resolution, SQL diagnostics, upgrade, removal, and signature rejection\n'

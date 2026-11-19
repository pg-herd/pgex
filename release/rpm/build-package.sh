#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 4 || $# -gt 5 ]]; then
  echo "Usage: $0 EXTRACTED_RELEASE VERSION ARCHITECTURE OUTPUT_DIRECTORY [RELEASE]" >&2
  exit 2
fi
input=$(realpath "$1")
version=$2
architecture=$3
mkdir -p "$4"
output=$(realpath "$4")
release=${5:-1}
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ && "$release" =~ ^[0-9]+$ ]]
case "$architecture" in x86_64|aarch64) ;; *) echo "Unsupported architecture: $architecture" >&2; exit 2 ;; esac
[[ "$(rpm --eval '%{_arch}')" == "$architecture" ]]
[[ -x "$input/pgex" && -f "$input/LICENSE" && -d "$input/licenses" ]]
spec=$(realpath "$(dirname "$0")/pgex.spec")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
cp -R "$input/." "$work/SOURCES/"
rpmbuild -bb --define "_topdir $work" --define "package_version $version" \
  --define "package_release $release" "$spec"
package="$work/RPMS/$architecture/pgex-$version-$release.$architecture.rpm"
rpm -qp --requires "$package" | grep -F 'libpq.so.5'
# Verify packaging did not change the executable.
mkdir "$work/extracted"
(cd "$work/extracted" && rpm2cpio "$package" | cpio -idm --quiet)
cmp "$input/pgex" "$work/extracted/usr/bin/pgex"
cp "$package" "$output/"

#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 4 ]]; then
  echo "Usage: $0 EXTRACTED_RELEASE VERSION ARCHITECTURE OUTPUT_DIRECTORY" >&2
  exit 2
fi
input=$(realpath "$1")
version=$2
architecture=$3
mkdir -p "$4"
output=$(realpath "$4")
dpkg --validate-version "$version"
case "$architecture" in amd64|arm64) ;; *) echo "Unsupported architecture: $architecture" >&2; exit 2 ;; esac
[[ "$(dpkg --print-architecture)" == "$architecture" ]]
[[ -x "$input/pgex" && -f "$input/LICENSE" && -d "$input/licenses" ]]

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
package="$work/debian/pgex"
install -Dm755 "$input/pgex" "$package/usr/bin/pgex"
install -Dm644 "$input/LICENSE" "$package/usr/share/doc/pgex/copyright"
install -m644 "$input/README.md" "$package/usr/share/doc/pgex/README.md"
cp -R "$input/licenses" "$package/usr/share/doc/pgex/licenses"

# Derive the runtime libraries and minimum versions from the released executable.
mkdir -p "$work/debian" "$package/DEBIAN"
printf 'Source: pgex\nBuild-Depends: libpq-dev\n\nPackage: pgex\nArchitecture: any\nDescription: PostgreSQL data refresh utility\n' > "$work/debian/control"
dependencies=$(cd "$work" && dpkg-shlibdeps -O -e debian/pgex/usr/bin/pgex)
dependencies=${dependencies#shlibs:Depends=}
[[ -n "$dependencies" && "$dependencies" == *libpq5* ]]
cat > "$package/DEBIAN/control" <<CONTROL
Package: pgex
Version: $version
Architecture: $architecture
Maintainer: Tomer Sela <tomersela@gmail.com>
Section: database
Priority: optional
Depends: $dependencies, ca-certificates
Installed-Size: $(du -sk "$package/usr" | cut -f1)
Homepage: https://github.com/pg-herd/pgex
Description: Refresh PostgreSQL tables with selected data
 Copy whole tables or selected rows using SQL conditions, reusable YAML
 configuration, and parallel transfers with a consistent source snapshot.
CONTROL
chmod -R a+rX "$package"
dpkg-deb --root-owner-group --build "$package" "$output/pgex_${version}_${architecture}.deb"

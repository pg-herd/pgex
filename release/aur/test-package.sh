#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 3 || $(id -u) != 0 || ! -f /.dockerenv ]]; then
  echo "Run in a disposable Arch container: $0 PKGBUILD RELEASE_ARCHIVE OUTPUT_DIRECTORY" >&2
  exit 2
fi
recipe=$(realpath "$1")
archive=$(realpath "$2")
mkdir -p "$3"
output=$(realpath "$3")
read -r -a pacman_flags <<< "${PACMAN_FLAGS:-}"
builder=$(realpath "$(dirname "$0")/../arch/build-package.sh")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
chmod 755 "$work"
bash "$builder" "$recipe" "$archive" "$output"
sed 's/^pkgrel=1$/pkgrel=0/' "$recipe" > "$work/previous.PKGBUILD"
bash "$builder" "$work/previous.PKGBUILD" "$archive" "$work/previous"
package=$(find "$output" -maxdepth 1 -name 'pgex-bin-*.pkg.tar.zst')
previous=$(find "$work/previous" -maxdepth 1 -name 'pgex-bin-*.pkg.tar.zst')
version=$(pacman -Qp "$package" | awk '{print $2}')
pacman "${pacman_flags[@]}" -Syu --noconfirm --needed postgresql-libs
pacman -U --noconfirm "$previous"
[[ "$(pacman -Q pgex-bin)" != "pgex-bin $version" ]]
pacman -U --noconfirm "$package"
[[ "$(pacman -Q pgex-bin)" == "pgex-bin $version" ]]
pacman -Qk pgex-bin
LD_BIND_NOW=1 pgex --help >/dev/null
[[ -f /usr/share/licenses/pgex-bin/LICENSE && -d /usr/share/licenses/pgex-bin/third-party ]]
set +e
pgex --source postgresql://localhost/source --target postgresql://localhost/target \
  --where 'id =' > "$work/diagnostic" 2>&1
status=$?
set -e
[[ $status == 2 ]]
grep -F 'Invalid --where predicate' "$work/diagnostic"
grep -F 'Line 1, column 5' "$work/diagnostic"
pacman -R --noconfirm pgex-bin
[[ ! -e /usr/bin/pgex ]]
printf 'PASS: AUR checksum verification, unchanged binary, installation, upgrade, SQL diagnostics, licenses, and removal\n'

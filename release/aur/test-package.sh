#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 3 || $(id -u) != 0 || ! -f /.dockerenv ]]; then
  echo "Run in a disposable Arch container: $0 PKGBUILD EXTRACTED_RELEASE OUTPUT_DIRECTORY" >&2
  exit 2
fi
recipe=$(realpath "$1")
input=$(realpath "$2")
mkdir -p "$3"
output=$(realpath "$3")
read -r -a pacman_flags <<< "${PACMAN_FLAGS:-}"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
chmod 755 "$work"
mkdir "$work/current" "$work/previous"
cp "$recipe" "$work/current/PKGBUILD"
sed 's/^pkgrel=1$/pkgrel=0/' "$recipe" > "$work/previous/PKGBUILD"
chown -R builder:builder "$work"
for kind in current previous; do
  (cd "$work/$kind" && runuser -u builder -- makepkg --cleanbuild --nodeps --force)
done
package=$(find "$work/current" -maxdepth 1 -name 'pgex-bin-*.pkg.tar.zst')
previous=$(find "$work/previous" -maxdepth 1 -name 'pgex-bin-*.pkg.tar.zst')
[[ -f "$package" && -f "$previous" ]]
version=$(pacman -Qp "$package" | awk '{print $2}')
tar -xOf "$package" usr/bin/pgex > "$work/pgex"
cmp "$input/pgex" "$work/pgex"
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
cp "$recipe" "$output/PKGBUILD"
(cd "$work/current" && runuser -u builder -- makepkg --printsrcinfo) > "$output/.SRCINFO"
cp "$package" "$output/"
printf 'PASS: AUR checksum verification, unchanged binary, installation, upgrade, SQL diagnostics, licenses, and removal\n'

#!/usr/bin/env bash
set -euo pipefail

[[ $# == 1 && -f /.dockerenv && $(id -u) == 0 ]] || {
  echo 'Run this test only as root in a disposable Docker container: test-installer.sh INSTALLER' >&2
  exit 2
}
installer=$(realpath "$1")
work=$(mktemp -d)
cp /etc/os-release "$work/os-release"
trap 'cat "$work/os-release" > /etc/os-release; rm -rf "$work"' EXIT
original_path=$PATH
mkdir "$work/bin"

refuse() {
  if sh "$installer" > "$work/output" 2>&1; then
    echo "Installer accepted unsupported platform: $1" >&2
    exit 1
  fi
  grep -F "$1" "$work/output"
  [[ ! -e /etc/apt/sources.list.d/pgex.sources && ! -e /etc/yum.repos.d/pgex.repo &&
     ! -e /etc/zypp/repos.d/pgex.repo && ! -e /etc/pacman.d/pgex.conf ]]
}

cat > "$work/bin/uname" <<'SH'
#!/bin/sh
case "$1" in -s) echo Linux ;; -m) echo riscv64 ;; *) exit 1 ;; esac
SH
chmod +x "$work/bin/uname"
export PATH="$work/bin:$original_path"
refuse 'pgex supports x86-64 and ARM64.'
sed -i 's/riscv64/aarch64/' "$work/bin/uname"
printf 'ID=arch\n' > /etc/os-release
refuse 'Arch Linux packages require x86-64.'
rm "$work/bin/uname"
printf 'ID=alpine\nVERSION_ID=3.23\n' > /etc/os-release
refuse 'APK installation requires Alpine 3.24 or newer.'
printf 'ID=unsupported\nVERSION_ID=1\n' > /etc/os-release
refuse 'Supported distributions:'
printf 'ID=rocky\nVERSION_ID=8.10\n' > /etc/os-release
refuse 'RPM installation requires RHEL-compatible version 9 or newer.'
printf '#!/bin/sh\necho "glibc 2.33"\n' > "$work/bin/getconf"
chmod +x "$work/bin/getconf"
printf 'ID=debian\nVERSION_ID=12\n' > /etc/os-release
refuse 'pgex requires glibc 2.34 or newer.'
printf 'ID=opensuse-leap\nVERSION_ID=15.6\n' > /etc/os-release
refuse 'pgex requires glibc 2.34 or newer.'
printf 'PASS: unsupported architecture, distribution, RPM baseline, and glibc are rejected before package configuration changes\n'

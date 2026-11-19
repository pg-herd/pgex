#!/bin/sh
set -eu

main() {
  if [ "$#" -ne 0 ]; then
    echo 'Usage: install.sh (no arguments)' >&2
    exit 2
  fi
  [ "$(uname -s)" = Linux ] || fail 'This installer supports Linux. On macOS, use Homebrew.'
  case "$(uname -m)" in
    x86_64|aarch64|arm64) ;;
    *) fail 'pgex supports x86-64 and ARM64.' ;;
  esac
  [ -r /etc/os-release ] || fail 'Cannot identify this Linux distribution.'
  # /etc/os-release is supplied by the operating system.
  # shellcheck source=/dev/null
  . /etc/os-release
  case "${ID:-}" in
    debian|ubuntu) manager=apt ;;
    fedora) manager=dnf ;;
    rhel|centos|rocky|almalinux)
      distribution_version=${VERSION_ID:-0}
      [ "${distribution_version%%.*}" -ge 9 ] || fail 'RPM installation requires RHEL-compatible version 9 or newer.'
      manager=dnf ;;
    *) fail 'Supported distributions: Debian, Ubuntu, Fedora, and RHEL-compatible systems (RHEL, CentOS Stream, Rocky, AlmaLinux) version 9 or newer.' ;;
  esac
  [ ! -e /run/ostree-booted ] || fail 'Immutable OSTree systems are not supported by this installer.'
  libc=$(getconf GNU_LIBC_VERSION 2>/dev/null) || fail 'pgex requires glibc 2.34 or newer.'
  libc=${libc#glibc }
  major=${libc%%.*}
  minor=${libc#*.}
  minor=${minor%%.*}
  if [ "$major" -lt 2 ] || { [ "$major" -eq 2 ] && [ "$minor" -lt 34 ]; }; then
    fail 'pgex requires glibc 2.34 or newer. Upgrade your distribution first.'
  fi
  command -v curl >/dev/null 2>&1 || fail 'Install curl, then run this installer again.'
  if [ "$(id -u)" -eq 0 ]; then
    privilege=
  elif command -v sudo >/dev/null 2>&1; then
    privilege=sudo
  else
    fail 'Run this installer as root, or install sudo.'
  fi
  temporary=$(mktemp -d)
  trap 'rm -rf "$temporary"' EXIT
  trap 'exit 1' HUP INT TERM
  base=https://pg-herd.github.io/pgex
  case "$manager" in
    apt)
      command -v apt-get >/dev/null 2>&1 || fail 'APT is not available on this system.'
      $privilege apt-get update
      $privilege apt-get install -y --no-install-recommends ca-certificates
      curl -fsSL "$base/apt/pgex.asc" -o "$temporary/pgex.asc"
      curl -fsSL "$base/apt/pgex.sources" -o "$temporary/pgex.sources"
      $privilege install -d -m0755 /etc/apt/keyrings
      $privilege install -m0644 "$temporary/pgex.asc" /etc/apt/keyrings/pgex.asc
      $privilege install -m0644 "$temporary/pgex.sources" /etc/apt/sources.list.d/pgex.sources
      $privilege apt-get update
      $privilege apt-get install -y pgex
      ;;
    dnf)
      command -v dnf >/dev/null 2>&1 || fail 'DNF is not available on this system.'
      $privilege dnf install -y ca-certificates
      curl -fsSL "$base/rpm/pgex.repo" -o "$temporary/pgex.repo"
      $privilege install -d -m0755 /etc/yum.repos.d
      $privilege install -m0644 "$temporary/pgex.repo" /etc/yum.repos.d/pgex.repo
      $privilege dnf install -y pgex
      ;;
  esac
  pgex --help >/dev/null
  echo 'pgex installed. Run pgex --help to get started.'
}

fail() {
  echo "pgex installer: $*" >&2
  exit 1
}

main "$@"

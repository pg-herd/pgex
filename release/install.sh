#!/bin/sh
set -eu

main() {
  if [ "$#" -ne 0 ]; then
    echo 'Usage: install.sh (no arguments)' >&2
    exit 2
  fi
  [ "$(uname -s)" = Linux ] || fail 'This installer supports Linux. On macOS, use Homebrew.'
  architecture=$(uname -m)
  case "$architecture" in
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
    opensuse-leap|opensuse-tumbleweed) manager=zypper ;;
    arch)
      [ "$architecture" = x86_64 ] || fail 'Arch Linux packages require x86-64.'
      manager=pacman ;;
    alpine)
      distribution_version=${VERSION_ID:-0}
      major=${distribution_version%%.*}
      minor=${distribution_version#*.}
      minor=${minor%%.*}
      if [ "$major" -lt 3 ] || { [ "$major" -eq 3 ] && [ "$minor" -lt 24 ]; }; then
        fail 'APK installation requires Alpine 3.24 or newer.'
      fi
      manager=apk ;;
    rhel|centos|rocky|almalinux)
      distribution_version=${VERSION_ID:-0}
      [ "${distribution_version%%.*}" -ge 9 ] || fail 'RPM installation requires RHEL-compatible version 9 or newer.'
      manager=dnf ;;
    *) fail 'Supported distributions: Debian, Ubuntu, Fedora, openSUSE Leap and Tumbleweed, Alpine 3.24+, Arch Linux, and RHEL-compatible systems (RHEL, CentOS Stream, Rocky, AlmaLinux) version 9 or newer.' ;;
  esac
  [ ! -e /run/ostree-booted ] || fail 'Immutable OSTree systems are not supported by this installer.'
  if [ "$manager" != apk ]; then
    libc=$(getconf GNU_LIBC_VERSION 2>/dev/null) || fail 'pgex requires glibc 2.34 or newer.'
    libc=${libc#glibc }
    major=${libc%%.*}
    minor=${libc#*.}
    minor=${minor%%.*}
    if [ "$major" -lt 2 ] || { [ "$major" -eq 2 ] && [ "$minor" -lt 34 ]; }; then
      fail 'pgex requires glibc 2.34 or newer. Upgrade your distribution first.'
    fi
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
    zypper)
      command -v zypper >/dev/null 2>&1 || fail 'Zypper is not available on this system.'
      curl -fsSL "$base/rpm/pgex.asc" -o "$temporary/pgex.asc"
      curl -fsSL "$base/rpm/pgex.repo" -o "$temporary/pgex.repo"
      printf '\nautorefresh=1\npkg_gpgcheck=1\n' >> "$temporary/pgex.repo"
      $privilege rpm --import "$temporary/pgex.asc"
      $privilege install -Dm0644 "$temporary/pgex.repo" /etc/zypp/repos.d/pgex.repo
      $privilege zypper --non-interactive refresh pgex
      $privilege zypper --non-interactive install --from pgex pgex
      ;;
    apk)
      command -v apk >/dev/null 2>&1 || fail 'APK is not available on this system.'
      $privilege apk add --no-cache ca-certificates
      curl -fsSL "$base/apk/pgex.rsa.pub" -o "$temporary/pgex.rsa.pub"
      $privilege install -d -m0755 /etc/apk/keys
      $privilege install -m0644 "$temporary/pgex.rsa.pub" /etc/apk/keys/pgex.rsa.pub
      if ! grep -Fxq "$base/apk" /etc/apk/repositories; then
        printf '%s\n' "$base/apk" | $privilege tee -a /etc/apk/repositories >/dev/null
      fi
      $privilege apk add --update-cache pgex
      ;;
    pacman)
      install_arch
      ;;
  esac
  pgex --help >/dev/null
  echo 'pgex installed. Run pgex --help to get started.'
}

install_arch() {
  command -v pacman >/dev/null 2>&1 || fail 'Pacman is not available on this system.'
  repositories=$(pacman-conf --repo-list)
  include='Include = /etc/pacman.d/pgex.conf'
  if printf '%s\n' "$repositories" | grep -Fxq pgex && ! grep -Fxq "$include" /etc/pacman.conf; then
    fail 'A pgex repository is already configured in pacman. Review it before running this installer.'
  fi

  fingerprint=EFD48C2B17A32B00855B52E00623CF316C7C43E4
  curl -fsSL "$base/arch/pgex.asc" -o "$temporary/pgex.asc"
  downloaded=$(gpg --batch --no-options --homedir "$temporary" --with-colons --show-keys "$temporary/pgex.asc" |
    awk -F: '$1 == "pub" {primary = 1} $1 == "fpr" && primary {print $10; primary = 0}')
  [ "$downloaded" = "$fingerprint" ] || fail 'The pacman repository signing key does not match the pgex fingerprint.'

  $privilege pacman-key --init
  $privilege pacman-key --add "$temporary/pgex.asc"
  $privilege pacman-key --lsign-key "$fingerprint"
  cat > "$temporary/pgex.conf" <<REPOSITORY
[pgex]
SigLevel = Required TrustedOnly
Server = $base/arch/\$arch
REPOSITORY
  $privilege install -Dm0644 "$temporary/pgex.conf" /etc/pacman.d/pgex.conf
  if ! grep -Fxq "$include" /etc/pacman.conf; then
    printf '\n%s\n' "$include" | $privilege tee -a /etc/pacman.conf >/dev/null
  fi
  # Keep pacman's interactive upgrade prompt off the installer script's stdin.
  if ( : </dev/tty ) 2>/dev/null; then
    $privilege pacman -Syu --needed pgex/pgex </dev/tty
  else
    $privilege pacman -Syu --needed --noconfirm pgex/pgex </dev/null
  fi
}

fail() {
  echo "pgex installer: $*" >&2
  exit 1
}

main "$@"

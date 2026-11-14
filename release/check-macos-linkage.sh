#!/usr/bin/env bash
set -euo pipefail

binary=$1
openssl_prefix=$(brew --prefix openssl@3)
[[ "$(lipo -archs "$binary")" == arm64 ]]

dependencies=$(otool -L "$binary")
printf '%s\n' "$dependencies"
for library in libssl.3.dylib libcrypto.3.dylib; do
    grep -Fq "$openssl_prefix/lib/$library " <<< "$dependencies"
    test -r "$openssl_prefix/lib/$library"
done

awk -v prefix="$openssl_prefix" '
    NR > 1 && $1 !~ /^\/(usr\/lib\/|System\/Library\/)/ &&
    $1 != prefix "/lib/libssl.3.dylib" && $1 != prefix "/lib/libcrypto.3.dylib" {
        print "Unexpected runtime dependency: " $1 > "/dev/stderr"
        failed = 1
    }
    END { exit failed }
' <<< "$dependencies"

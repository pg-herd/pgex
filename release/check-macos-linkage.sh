#!/usr/bin/env bash
set -euo pipefail

binary=$1
[[ "$(lipo -archs "$binary")" == arm64 ]]

dependencies=$(otool -L "$binary")
printf '%s\n' "$dependencies"
awk '
    NR > 1 && $1 !~ /^\/(usr\/lib\/|System\/Library\/)/ && $1 != "/opt/homebrew/opt/libpq/lib/libpq.5.dylib" {
        print "Unexpected runtime dependency: " $1 > "/dev/stderr"
        failed = 1
    }
    END { exit failed }
' <<< "$dependencies"

grep -F "/opt/homebrew/opt/libpq/lib/libpq.5.dylib" <<< "$dependencies" >/dev/null

#!/bin/sh
# Offline by default. A local Git checkout also verifies the reviewed manifest.
set -eu
export LC_ALL=C
crate=$(dirname "$0")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
pin=$(cat "$crate/upstream-revision")
case $# in
    0) ;;
    1|3)
        revision=$pin
        if [ "$#" -eq 3 ]; then
            [ "$2" = --revision ] || exit 2
            revision=$3
        fi
        git -C "$1" ls-tree -r "$revision" -- COPYRIGHT src/bin/pg_dump src/common/fe_memutils.c > "$work/tree"
        awk '{print $3, $4}' "$work/tree" > "$work/expected"
        diff -u "$crate/upstream-blobs.txt" "$work/expected"
        ;;
    *) echo "Usage: sh $0 [CHECKOUT [--revision REVISION]]" >&2; exit 2 ;;
esac
(cd "$crate/native/upstream" && find . ! -type d -print) > "$work/files"
sed 's|^\./||' "$work/files" > "$work/paths"
sort "$work/paths" > "$work/sorted"
awk '{print $2}' "$crate/upstream-blobs.txt" > "$work/list"
sort "$work/list" > "$work/tracked"
diff -u "$work/tracked" "$work/sorted"
while read -r blob path; do
    actual=$(git hash-object --no-filters "$crate/native/upstream/$path")
    if [ "$actual" != "$blob" ]; then
        echo "Vendored file differs from $pin: $path" >&2
        exit 1
    fi
done < "$crate/upstream-blobs.txt"
echo "Vendored files match the PG18.7 blob manifest."

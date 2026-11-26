#!/bin/sh
# Called by Cargo with the source and build directories used by the C compiler.
set -eu
crate=$(CDPATH='' cd -- "$(dirname "$0")" && pwd)
source_dir=$1
build_dir=$2
pin=$(cat "$crate/upstream-revision")

if [ ! -d "$source_dir" ]; then
    mkdir -p "$(dirname "$source_dir")"
    staging=$(mktemp -d "${source_dir}.XXXXXXXX")
    trap 'rm -rf "$staging"' EXIT
    git -C "$staging" init -q
    git -C "$staging" fetch --quiet --depth 1 https://github.com/postgres/postgres.git "$pin"
    git -C "$staging" checkout --quiet --detach FETCH_HEAD
    mv "$staging" "$source_dir"
    trap - EXIT
fi
test "$(git -C "$source_dir" rev-parse HEAD)" = "$pin" || {
    echo "Planner build requires PostgreSQL revision $pin in $source_dir" >&2
    exit 1
}
sh "$crate/verify_upstream.sh" "$source_dir"
source_dir=$(cd "$source_dir" && pwd)
cd "$build_dir"

# Respect compiler/linker search paths. Homebrew's keg-only OpenSSL needs a
# search path on macOS; ask Homebrew for it when that installation is available.
if [ "$(uname -s)" = Darwin ] && command -v brew >/dev/null; then
    if openssl=$(brew --prefix openssl@3) && [ -d "$openssl/include" ]; then
        export CPPFLAGS="${CPPFLAGS:+$CPPFLAGS }-I$openssl/include" LDFLAGS="${LDFLAGS:+$LDFLAGS }-L$openssl/lib"
    fi
    # pq-sys checks explicit paths, pkg-config and pg_config. A keg-only
    # libpq may have none of those available to Cargo.
    suffix=$(printf '%s' "$TARGET" | tr '[:lower:]-' '[:upper:]_')
    if [ -z "${PQ_LIB_DIR:-}" ] && ! printenv "PQ_LIB_DIR_$suffix" >/dev/null &&
       ! printenv "PG_CONFIG_$suffix" >/dev/null &&
       ! command -v pg_config >/dev/null && ! pkg-config --exists libpq 2>/dev/null &&
       pq=$(brew --prefix libpq) && [ -f "$pq/lib/libpq.dylib" ]; then
        printf 'cargo:rustc-link-search=native=%s/lib\n' "$pq"
    fi
fi
set -- --prefix="$(pwd)/install" --with-ssl=openssl --without-icu --without-readline --without-zlib
export CFLAGS="-O2 -fPIC${CFLAGS:+ $CFLAGS}"
configuration=$(printf '%s\n' "$source_dir" "$pin" "$@" "${CC:-}" "$CFLAGS" "${CPPFLAGS:-}" "${LDFLAGS:-}" "${SDKROOT:-}" "${MACOSX_DEPLOYMENT_TARGET:-}")
if [ ! -f config.status ] || [ "$configuration" != "$(cat .pgex-configuration 2>/dev/null || :)" ]; then
    "$source_dir/configure" "$@"
    printf '%s\n' "$configuration" > .pgex-configuration
fi

make -j"${NUM_JOBS:-1}" -C src/backend generated-headers
make -j"${NUM_JOBS:-1}" -C src/bin/pg_dump all
# Differential tests run the stock tool with its matching libpq.
make -C src/interfaces/libpq install
make -C src/bin/pg_dump install

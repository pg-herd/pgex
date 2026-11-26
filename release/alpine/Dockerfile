FROM rust:1.99.1-alpine3.24 AS build
ARG TARGETARCH
WORKDIR /build
RUN apk add --no-cache build-base clang-dev pkgconf libpq-dev
# Build tools such as bindgen must be able to load libclang, and the executable
# uses Alpine's shared libpq. Apply this to host build tools as well as pgex.
ENV RUSTFLAGS="-C target-feature=-crt-static" \
    PQ_LIB_DIR=/usr/lib \
    LIBCLANG_PATH=/usr/lib \
    CARGO_BUILD_JOBS=3 \
    CARGO_INCREMENTAL=0
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
COPY src/ src/
RUN set -eu; \
    case "$TARGETARCH" in \
        amd64) target=x86_64-unknown-linux-musl ;; \
        arm64) target=aarch64-unknown-linux-musl ;; \
        *) echo "Unsupported Alpine architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    rustc --version --verbose | grep -Fx "host: $target"; \
    cargo build --release --locked; \
    mkdir /out; \
    cp target/release/pgex /out/

FROM build AS test
COPY tests/ tests/
RUN cargo test --locked --no-run
CMD ["cargo", "test", "--locked", "--offline", "--", "--test-threads=2"]

FROM alpine:3.24 AS verify
RUN apk add --no-cache ca-certificates libgcc libpq \
    && addgroup -g 10001 pgex \
    && adduser -D -u 10001 -G pgex pgex
WORKDIR /out
COPY --from=build /out/pgex .
COPY LICENSE /usr/share/doc/pgex/LICENSE
COPY licenses/ /usr/share/doc/pgex/licenses/
RUN set -eu; \
    ldd ./pgex > runtime-dependencies.txt; \
    cat runtime-dependencies.txt; \
    if grep -F 'not found' runtime-dependencies.txt; then exit 1; fi; \
    grep -E 'ld-musl-(x86_64|aarch64)\.so\.1' runtime-dependencies.txt; \
    grep -F 'libpq.so.5' runtime-dependencies.txt; \
    LD_BIND_NOW=1 ./pgex --help >/dev/null
USER pgex
ENTRYPOINT ["/out/pgex"]
CMD ["--help"]

FROM scratch AS artifacts
COPY --from=verify /out/ /

FROM rust:1.99.1-bookworm AS build
ARG TARGETARCH
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src/ src/
RUN set -eu; \
    case "$TARGETARCH" in \
        amd64) target=x86_64-unknown-linux-gnu ;; \
        arm64) target=aarch64-unknown-linux-gnu ;; \
        *) echo "Unsupported Linux architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    cargo build --release --locked --target "$target"; \
    mkdir /out; \
    cp "target/$target/release/pgex" /out/

FROM debian:bookworm-slim AS verify
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libgcc-s1 libssl3 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /out
COPY --from=build /out/pgex .
RUN set -eu; \
    ldd ./pgex > runtime-dependencies.txt; \
    cat runtime-dependencies.txt; \
    if grep -F 'not found' runtime-dependencies.txt; then exit 1; fi; \
    grep -E '^[[:space:]]*libssl[.]so[.]3 => /' runtime-dependencies.txt; \
    grep -E '^[[:space:]]*libcrypto[.]so[.]3 => /' runtime-dependencies.txt; \
    LD_BIND_NOW=1 ./pgex --help >/dev/null

FROM scratch
COPY --from=verify /out/ /

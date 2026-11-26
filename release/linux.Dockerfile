FROM rust:1.99.1-bookworm AS build
ARG TARGETARCH
WORKDIR /build
ENV CARGO_INCREMENTAL=0
RUN apt-get update && apt-get install -y --no-install-recommends build-essential clang libclang-dev libpq-dev \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
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

FROM build AS test
COPY tests/ tests/
RUN cargo test --locked --no-run
CMD ["cargo", "test", "--locked", "--offline", "--", "--test-threads=2"]

FROM debian:bookworm-slim AS verify
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libgcc-s1 libpq5 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --user-group --create-home --shell /usr/sbin/nologin pgex
WORKDIR /out
COPY --from=build /out/pgex .
COPY LICENSE /usr/share/doc/pgex/LICENSE
COPY licenses/ /usr/share/doc/pgex/licenses/
RUN set -eu; \
    ldd ./pgex > runtime-dependencies.txt; \
    cat runtime-dependencies.txt; \
    if grep -F 'not found' runtime-dependencies.txt; then exit 1; fi; \
    grep -F 'libpq.so.5' runtime-dependencies.txt; \
    LD_BIND_NOW=1 ./pgex --help >/dev/null
USER pgex
ENTRYPOINT ["/out/pgex"]
CMD ["--help"]

FROM scratch AS artifacts
COPY --from=verify /out/ /

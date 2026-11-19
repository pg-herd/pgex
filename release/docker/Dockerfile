FROM debian:bookworm-slim

ARG VERSION
ARG REVISION
LABEL org.opencontainers.image.title="pgex" \
      org.opencontainers.image.description="Refresh PostgreSQL databases with selected data" \
      org.opencontainers.image.source="https://github.com/pg-herd/pgex" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.version="$VERSION" \
      org.opencontainers.image.revision="$REVISION"

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libgcc-s1 libpq5 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --user-group --create-home --shell /usr/sbin/nologin pgex
COPY --chmod=755 pgex /usr/local/bin/pgex
COPY LICENSE /usr/share/doc/pgex/LICENSE
COPY licenses/ /usr/share/doc/pgex/licenses/
RUN LD_BIND_NOW=1 /usr/local/bin/pgex --help >/dev/null

WORKDIR /work
USER pgex
ENTRYPOINT ["/usr/local/bin/pgex"]
CMD ["--help"]

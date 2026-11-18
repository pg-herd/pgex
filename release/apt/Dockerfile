FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    apt-utils binutils ca-certificates dpkg-dev gnupg libpq-dev \
    && rm -rf /var/lib/apt/lists/*

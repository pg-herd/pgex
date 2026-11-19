# Building and contributing to pgex

For installation, configuration, and usage, see the [README](README.md).

## Build from source

Install Rust through rustup and a C toolchain. The repository pins Rust 1.99.1 in
[rust-toolchain.toml](rust-toolchain.toml), including rustfmt and Clippy.

### Debian and Ubuntu

Install the native build dependencies:

```sh
sudo apt-get update
sudo apt-get install build-essential clang libclang-dev libpq-dev ca-certificates
cargo build --release --locked
./target/release/pgex --help
```

### macOS

Install Xcode or its Command Line Tools, rustup, and Homebrew libpq:

```sh
brew install libpq
export PQ_LIB_DIR="$(brew --prefix libpq)/lib"
export LIBCLANG_PATH="$(dirname "$(xcrun --find clang)")/../lib"
cargo build --release --locked
./target/release/pgex --help
```

Keep these variables set when running Cargo checks and tests. `PQ_LIB_DIR` locates
Homebrew's keg-only libpq; `LIBCLANG_PATH` locates libclang for the parser build.

### Native libraries

Database I/O uses unmodified libpq through `pq-sys`. Libpq supplies TLS through its
OpenSSL backend. SQL scanning and offline syntax validation use unmodified
`libpg_query`, built by the pinned `pg_query` package. The C adapter preserves
original predicate byte ranges without decoding a Rust AST.

The parser is statically linked; libclang is required at build time, not runtime.
Libpq remains a runtime dependency. The supported runtime packages are listed in
the README. No database driver or parser library is patched or vendored in this
repository.

## Checks and tests

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --lib --bin pgex       # Unit and CLI argument tests.
cargo test --locked --test integration    # PostgreSQL and end-to-end CLI tests.
cargo test --locked -- --test-threads=2    # Full suite, with limited test concurrency.
```

Start Docker or Docker Desktop before running integration tests. Testcontainers
starts disposable PostgreSQL 17 instances, generates TLS certificates and
passwords, and removes the containers afterward. The first run may pull the
pinned `postgres:17.11-bookworm` image. No existing database is required.

On macOS, the same tests can start disposable native PostgreSQL instances instead:

```sh
brew install postgresql@17
PGEX_TEST_POSTGRES_BIN="$(brew --prefix postgresql@17)/bin" \
  cargo test --locked -- --test-threads=2
```

This does not use a running PostgreSQL service. Each test initializes its own data
directory, certificate, password, and port, and stops its server on cleanup. An
invalid `PGEX_TEST_POSTGRES_BIN` fails the test; it does not fall back to Docker.
`PGEX_TEST_BINARY` optionally selects the executable used by CLI integration
tests, so release checks can exercise the optimized binary without compiling an
optimized version of every test.

The [CI workflow](.github/workflows/ci.yml) runs formatting, Clippy, and the full
suite on Linux glibc AMD64 for pull requests and code changes on `main`.
Documentation-only changes do not start the database suite. Superseded runs are
cancelled, and dependency/build caches are separated by platform and toolchain.
Changes to release builders, workflows, or database fixtures also run
[Release checks](.github/workflows/release-checks.yml), which exercises the same
five-target build/test matrix as a release, without publishing anything.

For behavior changes and bug fixes, add a regression test at the relevant unit
boundary. Small tests can live in an inline `#[cfg(test)] mod tests`. When setup
is substantial, keep the production module and put its tests in a child file,
following `src/snapshot/lease.rs` and `src/snapshot/lease/tests.rs`. PostgreSQL and
end-to-end CLI tests belong under `tests/integration/`.

Keep modules focused, with declarations and re-exports in `mod.rs`. Keep data
structures separate from orchestration and substantial processing logic. Native
C ownership and calls belong in the relevant backend directory, such as
`src/connection/libpq/` or `src/predicate/libpg_query/`.

Changes to connection handling and COPY must preserve cancellation, error details,
and rollback behavior. Run the exporter-disconnect and transfer rollback tests
as part of the full suite. Performance changes should be measured against the
same fixture and settings; passing correctness tests alone does not establish
a performance improvement.

## Build release archives

The [release workflow](.github/workflows/release.yml) uses the shared
[build/test workflow](.github/workflows/build.yml) for these targets:

| Target | Builder |
| --- | --- |
| `aarch64-apple-darwin` | Apple Silicon macOS 15 runner. |
| `x86_64-unknown-linux-gnu` | Native Linux x86-64 runner, Debian 12 builder image. |
| `aarch64-unknown-linux-gnu` | Native Linux ARM64 runner, Debian 12 builder image. |
| `x86_64-unknown-linux-musl` | Native Linux x86-64 runner, Alpine 3.24 builder image. |
| `aarch64-unknown-linux-musl` | Native Linux ARM64 runner, Alpine 3.24 builder image. |

All five targets run the full Rust suite using normal test builds before
publication, including rollback and exporter-disconnect tests. Tests run natively
on the corresponding architecture. The macOS runner uses temporary native
PostgreSQL 17 instances; Linux test runners use disposable Docker databases.

All targets use the pinned Rust toolchain and `Cargo.lock`. GNU Linux jobs use
[release/linux.Dockerfile](release/linux.Dockerfile) and check the executable in a
clean Debian 12 runtime image. Alpine jobs use
[release/alpine/Dockerfile](release/alpine/Dockerfile), with dynamic musl linking
(`RUSTFLAGS="-C target-feature=-crt-static"`) for both pgex and native build tools.
Both Linux builders expose a `test` stage and a minimal `verify` runtime. Each
optimized executable is checked in that runtime with verified TLS transfers,
parameter filtering, and COPY rollback. The binary in the runtime must match the
one packaged for release. The test runner shares the Linux Docker host's
network and sets `PGEX_TEST_HOST=localhost` to match the fixture's TLS certificate.
The macOS job checks architecture and dynamic library paths using
[release/check-macos-linkage.sh](release/check-macos-linkage.sh), then reruns CLI,
parameter-filtered COPY, and exporter-timeout/rollback checks against the optimized
executable. Cargo caches on macOS and BuildKit caches on Linux retain build work
between runs, with separate caches for each target and toolchain.

Run **Release binaries** manually in GitHub Actions to build and download verified
artifacts without publishing a release. Archives contain the executable,
`runtime-dependencies.txt`, README, contribution guide, `LICENSE`, and
the `licenses/` directory. Each archive
has a SHA-256 file.

To publish a release, update the Cargo package version and lockfile as needed,
then push a matching `v<version>` tag. For example, package version `0.1.0`
requires tag `v0.1.0`. The workflow publishes archives and checksums after all
builds pass. When updating the Rust pin, also update the Rust image versions in
the GNU Linux and Alpine Dockerfiles.

### Update Homebrew

Successful **Release binaries** runs automatically trigger the
[Homebrew bottle workflow](.github/workflows/homebrew.yml). It verifies the macOS
archive, prepares the formula, installs and tests the release, and publishes its
Apple Silicon macOS 15 bottle. Homebrew generates the bottle block from the
published metadata. The workflow then installs the published bottle and runs its
test before committing the updated `Formula/pgex.rb` to `main`.

The workflow uses GitHub's built-in token; no additional secret is needed. It can
also run manually for a published release using the optional `tag` input. Reruns
preserve existing bottle assets, and an older release never replaces the latest
stable version in the public formula. No manual checksum or formula edits are
needed for a new release.

### Publish Linux packages and repositories

The [Linux packages workflow](.github/workflows/linux-packages.yml) wraps the existing Linux
release executables in Debian and RPM packages. It does not rebuild pgex. RPM
packaging verifies that the executable is unchanged. Native x86-64 and ARM64 jobs
test the generic installer, dependency resolution, offline SQL diagnostics,
upgrades, removal, and signature rejection on Debian 12, Ubuntu 24.04,
Fedora 45, and Rocky Linux 9.

One-time setup:

1. In the repository's **Settings → Pages**, select **GitHub Actions** as the source.
2. Add the repository Actions secrets `APT_SIGNING_KEY` (the exported, encrypted
   private key) and `APT_SIGNING_PASSPHRASE` (its passphrase). These existing
   secrets sign both APT and RPM; no second signing key is needed.
3. Keep a password-manager backup of both. GitHub's settings do not reveal secret
   values after they are saved. The shared public key is tracked at
   [release/pgex.asc](release/pgex.asc); publishing checks that it matches the
   private signing key.

Run **Linux packages** manually to publish an existing release; the optional
`tag` input defaults to the latest stable release. Successful **Release binaries**
runs trigger it automatically for future releases. Pushes to package tooling run
tests without publishing.

The workflow signs new RPM packages before uploading them to their GitHub release.
Existing release assets remain unchanged. It collects all published Debian and RPM
packages, signs both repositories, and deploys `apt/`, `rpm/`, and `install.sh`
together to GitHub Pages. APT indexes, RPM metadata, and individual RPM packages
are signed. Production keys remain in the runner's temporary directory and are
never generated automatically by CI.

[release/install.sh](release/install.sh) selects APT or DNF from `/etc/os-release`.
It checks the architecture, glibc version, and distribution support before
changing package configuration. It supports root and sudo and installs only from
the signed repositories. Unsupported systems exit with a clear message.

### Publish Docker images

The [Docker image workflow](.github/workflows/docker.yml) downloads and verifies
an existing published Linux release, then builds a Debian-based runtime image on
native x86-64 and ARM64 runners. It checks that the executable is unchanged and
tests non-root/read-only operation, private YAML mounts, verified TLS, parameter
filtering, row refresh, and rollback against a disposable PostgreSQL instance.

The workflow publishes to `ghcr.io/pg-herd/pgex` using GitHub's built-in
`GITHUB_TOKEN` with `packages: write`; no registry password secret is needed.
On the first publication, make the container package **Public** in its GitHub
package settings so anonymous pulls work. The image is linked to this repository
through its OCI source label.

Run **Docker image** manually to publish an existing release; the optional `tag`
input defaults to the latest stable release. Successful **Release binaries** runs
also trigger it. After both architecture tests pass, the workflow publishes a
combined version tag such as `0.1.0`. It updates `latest` only when publishing the
current latest stable GitHub release. Architecture-specific tags such as
`0.1.0-amd64` and `0.1.0-arm64` are also available. Image labels identify the version
and the release's source commit. The runtime image includes `LICENSE` and the
third-party notices under `/usr/share/doc/pgex/`.

For a local image build, extract the appropriate release archive to `input/`:

```sh
docker build -f release/docker/Dockerfile -t pgex:test \
  --build-arg VERSION=0.1.0 --build-arg REVISION="$(git rev-parse 'v0.1.0^{commit}')" input
bash release/docker/test-image.sh pgex:test
```

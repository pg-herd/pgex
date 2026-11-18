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

The [release workflow](.github/workflows/release.yml) builds these targets:

| Target | Builder |
| --- | --- |
| `aarch64-apple-darwin` | Apple Silicon macOS 15 runner. |
| `x86_64-unknown-linux-gnu` | Native Linux x86-64 runner, Debian 12 builder image. |
| `aarch64-unknown-linux-gnu` | Native Linux ARM64 runner, Debian 12 builder image. |

All targets use the pinned Rust toolchain and `Cargo.lock`. Linux jobs use
[release/linux.Dockerfile](release/linux.Dockerfile) and check the executable in a
clean Debian 12 runtime image. The macOS job checks architecture and dynamic
library paths using [release/check-macos-linkage.sh](release/check-macos-linkage.sh),
then runs the executable's help command.

Run **Release binaries** manually in GitHub Actions to build and download verified
artifacts without publishing a release. Archives contain the executable,
`runtime-dependencies.txt`, README, contribution guide, `LICENSE`, and
the `licenses/` directory. Each archive
has a SHA-256 file.

To publish a release, update the Cargo package version and lockfile as needed,
then push a matching `v<version>` tag. For example, package version `0.1.0`
requires tag `v0.1.0`. The workflow publishes archives and checksums after all
builds pass. When updating the Rust pin, also update the Rust image version in
the Linux Dockerfile.

### Update Homebrew

After publishing a release, update `Formula/pgex.rb` with its version, macOS
archive URL, and SHA-256 from the release. Commit the formula to `main`; the tap
reads it from this repository.

Run the **Homebrew bottle** workflow to package the release for Homebrew. It
installs and tests the existing binary on a GitHub runner, then uploads the bottle
and its metadata to the release. Update the formula's `bottle` block with that
metadata. Verify installation with `brew reinstall pgex` and `brew test pgex`.

### Publish the APT repository

The [APT workflow](.github/workflows/apt.yml) packages existing Linux release
binaries; it does not rebuild pgex or change the parser. It tests installation,
SQL diagnostics, upgrades, removal, and rejection of untrusted signatures on
Debian 12 and Ubuntu 24.04, for both supported architectures.

One-time setup:

1. In the repository's **Settings → Pages**, select **GitHub Actions** as the source.
2. Add the repository Actions secrets `APT_SIGNING_KEY` (the exported, encrypted
   private key) and `APT_SIGNING_PASSPHRASE` (its passphrase).
3. Keep a password-manager backup of both. GitHub's settings do not reveal secret
   values after they are saved. The public key is tracked at
   [release/apt/pgex.asc](release/apt/pgex.asc); the workflow checks that it matches
   the private signing key.

Run **APT repository** manually to publish an existing release; the optional
`tag` input defaults to the latest stable release. Successful **Release binaries**
runs trigger it automatically for future releases. Pushes to the APT tooling run
package tests without publishing.

Packages are added to their GitHub release without replacing existing assets.
The workflow collects the published packages, creates and signs the APT indexes,
and deploys them to GitHub Pages. Private keys stay in the runner's temporary
directory. Production keys are never generated automatically by CI.

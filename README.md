# pgex

pgex refreshes tables from a PostgreSQL snapshot. For each table, it deletes
the destination rows in scope and streams their replacements using binary COPY.

## Build

Install Rust through rustup, a C toolchain, and the OpenSSL development
libraries. This repository pins Rust 1.99.1. On macOS with Homebrew, install
OpenSSL with `brew install openssl@3`. On Debian or Ubuntu, install
`build-essential`, `pkg-config`, and `libssl-dev`.

From the repository root, build the release binary and view the CLI help.

```sh
cargo build --release --locked
./target/release/pgex --help
```

## Release binaries

Download the `pgex-v<version>-<target>.tar.gz` archive for your platform from
GitHub Releases. OpenSSL is dynamically linked. Install the runtime packages
listed below before running the binary.

| Platform | Archive target | Runtime requirements |
| --- | --- | --- |
| macOS 15+ on Apple Silicon | `aarch64-apple-darwin` | macOS system libraries and Homebrew `openssl@3` under `/opt/homebrew`. Run `brew install openssl@3`. |
| Linux amd64 | `x86_64-unknown-linux-gnu` | Debian 12 (Bookworm) baseline, with glibc, `libgcc-s1`, `libssl3`, and `ca-certificates`. |
| Linux arm64 | `aarch64-unknown-linux-gnu` | Debian 12 (Bookworm) baseline, with the arm64 versions of the same runtime packages. |

On Debian 12, install the Linux runtime packages with
`sudo apt-get install libgcc-s1 libssl3 ca-certificates`. For another glibc-based
distribution, use its packages for the OpenSSL 3 shared libraries
(`libssl.so.3` and `libcrypto.so.3`) and check compatibility with `ldd ./pgex`.
The Linux archives require a glibc environment. On macOS, use
`otool -L ./pgex` to inspect the paths used by the loader. Private PostgreSQL
CAs can be supplied with `--ca` and `--target-ca`.

Each archive includes the build's `runtime-dependencies.txt`. To check the
archive before extracting, put its `.sha256` file next to it and run
`shasum -a 256 -c <archive>.sha256`. Run `./pgex --help` after extracting to
check that the loader can start the binary.

### Building a release

The [release workflow](.github/workflows/release.yml) uses an Apple Silicon
runner and one native Linux runner per architecture. Both Linux jobs build
with the [Debian 12 Dockerfile](release/linux.Dockerfile) and run the executable
in a clean Debian 12 runtime image. The macOS job checks the binary's architecture
and dynamic library paths, then runs it on macOS. All three build with the pinned
Rust toolchain and `Cargo.lock`.

Run **Release binaries** manually in GitHub Actions to build and download the
verified artifacts. Pushing a tag that matches the Cargo package version
(for example, `v0.1.0`) publishes the three archives and their SHA-256 files
after all builds pass. When updating the Rust pin, update the Linux builder's
image version in the Dockerfile too.

## Connect to the source and destination

Supply both `--source` and `--target` as PostgreSQL URLs or libpq-style
connection strings. Use TLS-enabled servers and the hostnames on their
certificates.

```sh
./target/release/pgex \
  --source 'postgresql://sync_user@source.example.com/app' \
  --target 'postgresql://sync_user@target.example.com/app'
```

For each side, the CLI username (`--source-user` or `--target-user`) takes
precedence over the YAML username (`source.connection.user` or
`target.connection.user`), then the username in that side’s URL.

pgex checks that the server certificate matches the hostname and trusts its
issuer through the OpenSSL trust store. If the issuer is missing from that
store, supply a PEM CA file with `--ca /path/to/ca.pem`. For Amazon RDS, use
the CA bundle for your instance.
[Amazon RDS PostgreSQL TLS documentation](https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/PostgreSQL.Concepts.General.SSL.html)

### Password

pgex resolves the password independently for each connection, in this order.

| Priority | Source | Destination |
| --- | --- | --- |
| 1 | `--source-password` | `--target-password` |
| 2 | `source.connection.password` in YAML | `target.connection.password` in YAML |
| 3 | `PGEX_SOURCE_PASSWORD` | `PGEX_TARGET_PASSWORD` |
| 4 | Password in the source connection string | Password in the target connection string |

If none is supplied, pgex attempts to connect without a password.

In CI, expose the password as the `PGEX_SOURCE_PASSWORD` environment variable
from your secret store and keep the command line password-free.

### Table selection

Without a filter, pgex refreshes every row in each selected table. Use
`--where` to set a shared SQL condition, or `--config` to supply table-specific
filters and column rules. Pass filter values with repeatable `--param name=value`
options. By default, pgex applies the effective source filter to the destination
DELETE. Use `--target-skip-delete` to keep existing destination rows.

`--tables` accepts one comma-separated list of `schema.table` names, with no
spaces around commas. The snapshot check holds read locks on the listed
tables until it finishes. When `--tables` is omitted, pgex discovers and
locks ordinary tables and partition roots across non-system schemas.
After importing the snapshot, pgex reads the selected table layouts on
the reader session while the exporter still holds the locks.

```sh
./target/release/pgex \
  --source 'postgresql://sync_user@source.example.com/app' \
  --target 'postgresql://sync_user@target.example.com/app' \
  --ca /path/to/ca.pem \
  --where 'account_id = ANY({{accounts:bigint[]}})' \
  --param accounts=1,2,3 \
  --tables public.events,public.documents
```

The PostgreSQL destination holds table locks on a dedicated connection and
compares the layouts before entering the transfer step. Use
`--target-validate-layouts false` to skip the comparison. The destination
still locks the selected tables. Pause writes to the destination rows being
refreshed. A successful run prints `Sync complete.`

Use `--target-ca` if the destination's CA is outside the OpenSSL trust store.

### Disconnect checks

Snapshot transactions ask PostgreSQL to check for closed client connections
every 1,000 milliseconds while a query is running. Use
`--client-connection-check-interval-ms 250` to override the interval or `0` to
disable the checks. The setting is local to each snapshot transaction and
requires PostgreSQL 14 or later. A nonzero interval requires server platform
support for connection polling.

### Source snapshot timeouts

| CLI option | Limit in milliseconds |
| --- | --- |
| `--source-exporter-transaction-timeout-ms` | Total lifetime of the snapshot export transaction |
| `--source-exporter-idle-in-transaction-timeout-ms` | Time the exporter can wait between commands in that transaction |
| `--source-reader-transaction-timeout-ms` | Total lifetime of each snapshot reader transaction |
| `--source-reader-idle-in-transaction-timeout-ms` | Time a reader can wait between commands in its transaction |

Each timeout is disabled by default on pgex's source snapshot connections,
even if the PostgreSQL role has a timeout configured. Pass a positive value
to enable it. Transaction lifetime limits require PostgreSQL 17 or later.
Reader options apply to the metadata reader and the
transfer readers. The exporter stays idle while readers use the snapshot, so
its idle limit must allow that part of the sync to finish. If the exporter
disconnects during the sync, pgex stops the workers and closes their
connections. Earlier destination commits remain applied.

### Source reader limit

Use `--source-reader-limit` to set the maximum number of concurrent source
reader sessions for a job. The default is 8. The snapshot exporter uses a
separate session. Reading source layouts opens one source reader.

### PostgreSQL transfers

Use `--target-parallel-transfers` to set how many PostgreSQL workers can run
at once. The default is 8. If the requested count exceeds the source reader
limit, pgex warns and uses the lower limit. Each worker imports the snapshot
once and keeps one destination connection across transactions. The destination
lock uses a separate connection.

Workers take one table at a time from the shared queue. After each write, the
worker adds the destination payload bytes to the transaction's running total.
It claims another table while the total is below the target. When a table
brings the total to or above the target, the worker commits before claiming
another table. Use `--target-commit-group-mib` to change the 32 MiB default.
A failed transaction rolls back that group's deletes and copies. Earlier
committed groups remain applied.

Pass `--target-skip-delete` to insert selected source rows without deleting
destination rows first. COPY uses normal insert semantics, so a duplicate key
or other constraint violation can fail the commit group.

Destination triggers and foreign keys run by default. To use the POC's
replica trigger behavior, pass `--target-replica-role`. This requires
permission to set `session_replication_role` and disables ordinary triggers,
including foreign-key checks, during each destination commit group.

Row security follows each PostgreSQL connection's settings by default. Use
`--source-row-security-off` to set `row_security = off` for each source
worker's snapshot transaction, or `--target-row-security-off` to set it for
each destination worker's connection. With this setting, PostgreSQL returns
an error if a statement would be subject to a row-level security policy.

## Tests

Add a regression test for behavior changes and bug fixes. Put small unit tests
in an inline `#[cfg(test)] mod tests`. For tests with more setup, keep the named
production module file and put its tests in a child file, as in
`src/snapshot/lease.rs` and `src/snapshot/lease/tests.rs`. Use Mockall to verify
interactions at an existing dependency boundary. Put PostgreSQL and end-to-end
CLI tests in the Cargo integration target under `tests/integration/`.

```sh
cargo test --locked --lib --bin pgex  # Unit tests
cargo test --locked --test integration  # Integration tests
cargo test --locked                    # Both
```

Start Docker or Docker Desktop for the integration target. Testcontainers starts
a disposable PostgreSQL 17 container for each integration test, generates its
TLS certificate and password, and removes it afterward. Docker pulls the pinned
`postgres:17.11-bookworm` image on the first run if needed.

## License

pgex is licensed under the [MIT License](LICENSE).

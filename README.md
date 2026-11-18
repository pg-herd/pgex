# pgex

pgex refreshes tables from PostgreSQL. For each table, it deletes
the destination rows in scope and streams their replacements using text COPY.
Worker sessions use ISO dates, PostgreSQL interval formatting, full floating-point
precision, and the C monetary locale to preserve values across connection defaults.

## Build

Install Rust through rustup and a C toolchain. This repository pins Rust 1.99.1.
All database I/O uses unmodified libpq through the maintained `pq-sys` bindings.
Install `build-essential`, `libpq-dev`, and `ca-certificates` on Debian or Ubuntu.
On macOS, run `brew install libpq` and set
`export PQ_LIB_DIR="$(brew --prefix libpq)/lib"` before building.
libpq supplies TLS through its native OpenSSL backend; no database driver is
vendored or patched.

From the repository root, build the release binary and view the CLI help.

```sh
cargo build --release --locked
./target/release/pgex --help
```

## Release binaries

Download the `pgex-v<version>-<target>.tar.gz` archive for your platform from
GitHub Releases. Install the runtime packages listed below before running the binary.

| Platform | Archive target | Runtime requirements |
| --- | --- | --- |
| macOS 15+ on Apple Silicon | `aarch64-apple-darwin` | macOS system libraries and Homebrew `libpq` (`brew install libpq`). |
| Linux amd64 | `x86_64-unknown-linux-gnu` | Debian 12 (Bookworm) baseline, with glibc, `libgcc-s1`, `libpq5`, and `ca-certificates`. |
| Linux arm64 | `aarch64-unknown-linux-gnu` | Debian 12 (Bookworm) baseline, with the arm64 versions of the same runtime packages. |

On Debian 12, install the Linux runtime packages with
`sudo apt-get install libgcc-s1 libpq5 ca-certificates`. On another glibc-based
distribution, check runtime compatibility with `ldd ./pgex`.
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
issuer through libpq/OpenSSL's system certificate roots. If the issuer is missing from that
store, supply a PEM CA file with `--ca /path/to/ca.pem`. For Amazon RDS, use
the CA bundle for your instance.
[Amazon RDS PostgreSQL TLS documentation](https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/PostgreSQL.Concepts.General.SSL.html)

Connection strings are parsed by libpq itself. Native options such as host
lists, paired ports, `hostaddr`, startup `options`, `target_session_attrs`, and
TCP keepalives are passed through. `connect_timeout` supplies the connection
attempt's deadline; without a nonzero value, pgex uses 30 seconds.
pgex always requires verified TLS, including when the URL requests a weaker mode.
It uses `sslrootcert=system` with libpq 16 or newer; on libpq 15 it uses the
platform CA bundle. Supply `--ca` or `--target-ca` for a private issuer.
The PostgreSQL server version is independent of the installed libpq client version.

### Password

pgex resolves the password independently for each connection, in this order.

| Priority | Source | Destination |
| --- | --- | --- |
| 1 | `--source-password` | `--target-password` |
| 2 | `source.connection.password` in YAML | `target.connection.password` in YAML |
| 3 | `PGEX_SOURCE_PASSWORD` | `PGEX_TARGET_PASSWORD` |
| 4 | Password in the source connection string | Password in the target connection string |

If none is supplied, pgex attempts to connect without a password; ambient
`PGPASSWORD` and implicit password files do not replace the resolved credentials.

In CI, expose the password as the `PGEX_SOURCE_PASSWORD` environment variable
from your secret store and keep the command line password-free.

### Table selection

Without a filter, pgex refreshes every row in each selected table. Use
`--where` to set a shared SQL condition, or `--config` to supply table-specific
filters and column rules. Pass filter values with repeatable `--param name=value`
options. By default, pgex applies the effective source filter to the destination
DELETE. Use `--target-skip-delete` to keep existing destination rows.

`--tables` accepts one comma-separated list of `schema.table` names, with no
spaces around commas. pgex holds read locks on the selected source tables
until the run finishes and reads their layouts while those locks are held.
When `--tables` is omitted, pgex discovers and locks ordinary tables and
partition roots across non-system schemas.

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
`--target-validate-layouts false` to skip the comparison. Use
`--no-target-locks` (YAML `target.transfer.no_locks: true`) to skip the
job-wide destination locks. Layout validation remains enabled by default.
Avoid destination schema changes for the duration of an unlocked transfer,
and pause writes to the destination rows being refreshed.
A successful run prints `Sync complete.`

Use `--target-truncate` (YAML `target.transfer.truncate: true`) to replace
destination DELETE with `TRUNCATE ... CASCADE` before each table’s COPY.
This mode skips the job-wide destination locks and commits each truncate
before COPY starts. COPY commits per table. A source WHERE filter still
limits the incoming rows, but TRUNCATE removes all existing rows in the
destination table. CASCADE also empties tables that reference it through
foreign keys, including tables outside the selection or tables another
worker already loaded. If COPY fails, the truncate stays committed. Use
distinct source and destination tables so source locks can coexist with
destination truncation.

Use `--target-ca` if the destination's CA is outside the system certificate trust store.

### Source snapshot

By default, all source tables are copied from one shared snapshot. Pass
`--source-snapshot per-statement` to give each source COPY its own snapshot.
On a live source, different tables can then see different commits. pgex holds
source table locks in both modes.

Set `source.snapshot.mode` to `shared` or `per-statement` in YAML. An explicit
CLI value takes precedence. For example, `--source-snapshot shared` overrides
`per-statement` in YAML.

### Disconnect checks

Source transactions ask PostgreSQL to check for closed client connections
every 1,000 milliseconds while a query is running. Use
`--client-connection-check-interval-ms 250` to override the interval or `0` to
disable the checks. The setting is local to each source transaction and
requires PostgreSQL 14 or later. A nonzero interval requires server platform
support for connection polling.

### Source transaction timeouts

| CLI option | Limit in milliseconds |
| --- | --- |
| `--source-exporter-transaction-timeout-ms` | Total lifetime of the source table lock transaction |
| `--source-exporter-idle-in-transaction-timeout-ms` | Time the source table lock session can wait between commands |
| `--source-reader-transaction-timeout-ms` | Total lifetime of each source reader transaction |
| `--source-reader-idle-in-transaction-timeout-ms` | Time a reader can wait between commands in its transaction |

Each timeout is disabled by default on pgex's source connections,
even if the PostgreSQL role has a timeout configured. Pass a positive value
to enable it. Transaction lifetime limits require PostgreSQL 17 or later.
Reader options apply to the metadata reader and the transfer readers. The
table lock session stays idle during transfer, so its idle limit must allow
that part of the sync to finish. If it disconnects, pgex stops the workers
and closes their connections. Earlier destination commits remain applied.

### Source reader limit

Use `--source-reader-limit` to set the maximum number of concurrent source
reader sessions for a job. The default is 8. Source table locks use a separate
session. Reading source layouts opens one source reader.

### PostgreSQL transfers

Use `--target-parallel-transfers` to set how many PostgreSQL worker threads
can run at once. The default is 8. If the requested count exceeds the source
reader limit, pgex warns and uses the lower limit. Each worker runs on its own
OS thread and keeps one source connection and one destination connection
across transfers. In shared mode, each worker imports the snapshot once. The destination lock uses a separate connection.

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
worker's source transaction, or `--target-row-security-off` to set it for
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

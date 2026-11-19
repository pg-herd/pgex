<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/pgex-logo-dark.svg">
    <img src="assets/pgex-logo.svg" alt="pgex">
  </picture>
</h1>

Refresh a PostgreSQL database with exactly the data you need.

Copy whole tables or select rows with SQL conditions. Save reusable settings in
YAML, supply parameters from the command line, and copy multiple tables in
parallel. By default, every source table is read from the same snapshot, so your
dataset reflects one point in time.

Use pgex to refresh a development database, prepare a test dataset, or copy a
selected slice of data into another PostgreSQL database.

- **Choose your data:** whole tables, a shared WHERE condition, or individual table filters.
- **Reuse your setup:** YAML configuration with explicit command-line changes.
- **Check inputs early:** SQL syntax and parameter validation with errors pointing to the condition.
- **Control the refresh:** replace matching rows, append rows, or truncate before copying.

[Install](#install) · [Quick start](#quick-start) · [Filter rows](#filter-rows) ·
[YAML configuration](#yaml-configuration) · [All options](#all-options) ·
[Build and contribute](CONTRIBUTING.md)

## Install

### Homebrew

On Apple Silicon Macs running macOS 15 or newer:

```sh
brew tap pg-herd/pgex https://github.com/pg-herd/pgex
brew trust --formula pg-herd/pgex/pgex
brew install pgex
```

The tap and formula trust are a one-time setup. Homebrew installs the required
PostgreSQL client library automatically. To update pgex later, run `brew update && brew upgrade pgex`.

### Linux

On Debian, Ubuntu, Fedora, and RHEL-compatible systems, with `curl` installed:

```sh
curl -fsSL https://pg-herd.github.io/pgex/install.sh | sh
```

The installer detects your distribution, adds the signed APT or RPM repository,
and installs pgex and its runtime libraries. It uses `sudo` when needed. Updates
then arrive through your normal package manager.

Installation is tested on Debian 12, Ubuntu 24.04, Fedora 45, and Rocky Linux 9,
for x86-64 and ARM64. RPM packages also target RHEL-compatible systems version 9
or newer, including RHEL, CentOS Stream, and AlmaLinux. APT and RPM packages require
glibc 2.34 or newer. For Alpine Linux 3.24 or newer, use the musl archives under
[Download a release](#download-a-release). The installer reports unsupported
distributions or architectures before changing package configuration.

<details>
<summary>Manual APT setup (Debian and Ubuntu)</summary>

Add the pgex repository once:

```sh
sudo apt-get update
sudo apt-get install -y ca-certificates curl
sudo install -d -m0755 /etc/apt/keyrings
sudo curl -fsSLo /etc/apt/keyrings/pgex.asc https://pg-herd.github.io/pgex/apt/pgex.asc
sudo curl -fsSLo /etc/apt/sources.list.d/pgex.sources https://pg-herd.github.io/pgex/apt/pgex.sources
sudo apt-get update
sudo apt-get install pgex
```

To update, use `sudo apt-get update && sudo apt-get install --only-upgrade pgex`.
To remove pgex, use `sudo apt-get remove pgex`.

</details>

<details>
<summary>Manual RPM setup (Fedora and RHEL-compatible systems)</summary>

With `curl` installed, add the repository once:

```sh
sudo curl -fsSLo /etc/yum.repos.d/pgex.repo https://pg-herd.github.io/pgex/rpm/pgex.repo
sudo dnf install pgex
```

DNF verifies both the repository metadata and the packages. To update, use
`sudo dnf upgrade pgex`. To remove pgex, use `sudo dnf remove pgex`.

</details>

### Docker

The image includes pgex and its runtime libraries, for Linux x86-64 and ARM64:

```sh
docker run --rm ghcr.io/pg-herd/pgex:latest --help
```

To use `pgex.yml` and any private CA files in your current directory, with the
password environment variables exported as in [Quick start](#quick-start):

```sh
docker run --rm \
  --user "$(id -u):$(id -g)" \
  --mount "type=bind,src=$PWD,dst=/config,readonly" \
  --workdir /config \
  --env PGEX_SOURCE_PASSWORD \
  --env PGEX_TARGET_PASSWORD \
  ghcr.io/pg-herd/pgex:latest --config pgex.yml
```

This Linux/macOS shell example uses your user ID so pgex can read private config
files without changing their permissions. Specify database usernames in the YAML
or connection URLs. CA file paths must refer to files inside the container.
For databases on a Docker network, add `--network NETWORK` and use their service
names as hostnames.

The image runs as the non-root `pgex` user by default. All normal pgex flags are
available after the image name. The `latest` tag follows the latest stable release.
To upgrade, pull the updated image, then repeat your `docker run` command:

```sh
docker pull ghcr.io/pg-herd/pgex:latest
```

To pin a specific release, replace `latest` with a version tag such as `0.1.1`.
To upgrade a pinned release, change that tag to the desired version.

### Download a release

Download the archive matching your platform from [GitHub Releases](https://github.com/pg-herd/pgex/releases),
extract it, and put the `pgex` executable on your PATH.

| Platform | Archive target | Required runtime packages |
| --- | --- | --- |
| macOS 15+ on Apple Silicon | `aarch64-apple-darwin` | Homebrew `libpq`: `brew install libpq`. |
| Linux x86-64 (glibc) | `x86_64-unknown-linux-gnu` | On Debian 12: `sudo apt-get install libgcc-s1 libpq5 ca-certificates`. |
| Linux ARM64 (glibc) | `aarch64-unknown-linux-gnu` | The same packages, for ARM64. |
| Alpine 3.24+ x86-64 | `x86_64-unknown-linux-musl` | `apk add --no-cache libgcc libpq ca-certificates`. |
| Alpine 3.24+ ARM64 | `aarch64-unknown-linux-musl` | The same packages, for ARM64. |

The GNU archives use Debian 12 as their baseline; the musl archives use Alpine 3.24.
Each archive includes `runtime-dependencies.txt` and the third-party license files
listed under [License](#license). To verify a download, place its
`.sha256` file alongside it and run `sha256sum -c <archive>.sha256` on Linux,
or `shasum -a 256 -c <archive>.sha256` on macOS. To upgrade a manual installation,
repeat the download and verification steps and replace the installed executable.

Run `pgex --help` to check the installation. For a source build, see
[Building and contributing](CONTRIBUTING.md).

## Quick start

The source and destination tables must already exist. Pgex copies rows; it does
not create schemas or synchronize sequence counters. Use PostgreSQL 14 or newer
with TLS enabled. Source credentials need permission to read the selected tables;
destination credentials need permission to read, insert, and delete their rows.

```sh
export PGEX_SOURCE_PASSWORD='source-password'
export PGEX_TARGET_PASSWORD='target-password'

pgex \
  --source 'postgresql://sync_user@source.example.com/app' \
  --target 'postgresql://sync_user@target.example.com/app' \
  --tables public.orders,public.order_items
```

This refreshes every row in the two destination tables. Omit `--tables` to select
all ordinary source tables and partition roots in non-system schemas. A successful
run prints `Sync complete.`

Source and destination layouts are checked before copying. See
[Refresh behavior](#refresh-behavior) for transaction and failure behavior.

### Connections and credentials

Both connections require verified TLS. For a private certificate authority, use
`--ca /path/to/source-ca.pem` and `--target-ca /path/to/target-ca.pem` independently.
Otherwise, pgex uses the system trust store. The hostname must match the server's
certificate; requesting a weaker TLS mode in a connection URL does not disable
verification.

The CLI accepts PostgreSQL URLs or libpq keyword/value connection strings. YAML
`connection.url` fields require a `postgresql://` or `postgres://` URL. Connection
URL options can specify host lists, paired ports, `hostaddr`, startup `options`,
`target_session_attrs`, TCP keepalives, and other native libpq settings. The
connection deadline is `connect_timeout` seconds, or 30 seconds when it is absent
or zero. Pgex enforces verified TLS and disables GSS encryption in favor of TLS.

For each side, the username is chosen in this order:

1. `--source-user` or `--target-user`.
2. YAML `source.connection.user` or `target.connection.user`.
3. The username in the resolved connection URL.
4. The operating-system username.

An omitted database name defaults to the resolved username. `PGUSER` and
`PGDATABASE` are ignored.

Passwords are resolved independently for the source and destination:

| Priority | Source | Destination |
| --- | --- | --- |
| 1 | `--source-password` | `--target-password` |
| 2 | YAML `source.connection.password` | YAML `target.connection.password` |
| 3 | `PGEX_SOURCE_PASSWORD` | `PGEX_TARGET_PASSWORD` |
| 4 | Password in the resolved source URL | Password in the resolved target URL |

Without a password, pgex attempts passwordless authentication. Ambient
`PGPASSWORD` and implicit password files do not supply credentials. In CI, provide
`PGEX_SOURCE_PASSWORD` and `PGEX_TARGET_PASSWORD` through your secret store.

## Filter rows

Give `--where` a SQL condition without the `WHERE` keyword. Use named parameters
for values:

```sh
pgex \
  --source 'postgresql://sync_user@source.example.com/app' \
  --target 'postgresql://sync_user@target.example.com/app' \
  --tables public.orders \
  --where 'tenant_id = ANY({{tenants:bigint[]}}) AND created_at >= {{since:date}}' \
  --param tenants=42,43 \
  --param since=2026-01-01
```

By default, the same condition selects source rows and destination rows to delete.
Rows outside that condition remain in the destination. A YAML
`target.delete_filters` entry can give a table a different deletion condition.

Conditions can contain subqueries and PostgreSQL functions. SQL syntax and
parameter values are checked before connecting. PostgreSQL checks referenced
objects and SQL type compatibility when it executes the condition.

### Parameters

A placeholder is `{{name}}` or `{{name:type}}`. Names start with an ASCII letter or
underscore and contain only ASCII letters, digits, and underscores. Names and type
annotations are case-sensitive. Placeholders are recognized outside SQL strings,
quoted identifiers, and comments; braces inside those stay literal text. A
placeholder supplies a value, not a table name, column name, or SQL fragment.

| Type | Example value | Accepted form |
| --- | --- | --- |
| `text` (also the default when omitted) | `hello` | Text. |
| `integer` | `42` | Signed 32-bit integer. |
| `bigint` | `42` | Signed 64-bit integer. |
| `boolean` | `true` | `true` or `false`. |
| `date` | `2026-01-01` | Valid `YYYY-MM-DD` date, years 0001–9999. |
| `integer[]` | `1,2,3` | Nonempty comma-separated list of 32-bit integers. |
| `bigint[]` | `1,2,3` | Nonempty comma-separated list of 64-bit integers. |

An untyped placeholder is text; declare a type or use an explicit SQL cast when
another type is needed. One named value can appear multiple times, with different
types at different occurrences, provided it is valid for every occurrence.

Supply values through repeatable `--param NAME=VALUE` arguments or a YAML `params`
map. YAML parameter values must be strings: quote numbers, booleans, dates, and
array values. CLI values replace YAML values with the same name. Missing, invalid,
duplicate, or unused parameter values are errors. NUL characters are rejected.

## YAML configuration

Run a saved configuration with:

```sh
pgex --config pgex.yml
```

This example shows every supported configuration field. Most are optional; their
normal defaults are shown for the snapshot, session, and transfer settings. The
selection and deletion conditions illustrate a tenant-specific refresh. Remove
optional credential and CA lines you do not need.

```yaml
source:
  connection:
    url: "postgresql://source.example.com/app"
    user: sync_user
    # password: "source-password"  # Or use PGEX_SOURCE_PASSWORD.
    # ca_file: "/path/to/source-ca.pem"  # Omit for the system trust store.
  session:
    row_security_off: false
  snapshot:
    mode: shared
    reader_connection_limit: 8
    client_connection_check_interval_ms: 1000
    exporter_timeouts:
      transaction_ms: 0
      idle_in_transaction_ms: 0
    reader_timeouts:
      transaction_ms: 0
      idle_in_transaction_ms: 0
  selection:
    tables:
      - public.orders
      - public.order_items
      - public.products
    where: "TRUE"
    table_rules:
      - has_column: tenant_id
        where: "tenant_id = ANY({{tenants:bigint[]}})"
    table_filters:
      public.orders:
        where: >-
          tenant_id = ANY({{tenants:bigint[]}})
          AND created_at >= {{since:date}}

target:
  connection:
    url: "postgresql://target.example.com/app"
    user: sync_user
    # password: "target-password"  # Or use PGEX_TARGET_PASSWORD.
    # ca_file: "/path/to/target-ca.pem"  # Omit for the system trust store.
  session:
    row_security_off: false
    replica_role: false
  transfer:
    workers: 8
    commit_group_mib: 32
    validate_layouts: true
    no_locks: false
    skip_delete: false
    truncate: false
  delete_filters:
    public.orders: "tenant_id = ANY({{tenants:bigint[]}})"

params:
  tenants: "42,43"
  since: "2026-01-01"
```

In this example, `public.orders` copies rows for the selected tenants since the
chosen date, but deletes all destination rows for those tenants. Other tables
with `tenant_id` use the column rule. Tables without that column use `TRUE`,
which selects every row.

Unknown YAML fields are rejected. File paths, including `ca_file`, are relative
to the current working directory. Explicit table names use simple `schema.table`
identifiers; an explicit table list must be nonempty and contain no duplicates.
Omitting the list enables discovery, including catalog names requiring SQL quoting.

### Which settings win?

Explicit CLI settings take precedence over the corresponding YAML fields; omitted
CLI settings leave YAML values intact. Defaults apply only after both inputs are
combined. `--tables` replaces the YAML table list. Credentials remain distinct
fields: changing a URL does not discard a separately configured username or password.

For example:

```sh
pgex --config pgex.yml \
  --param tenants=99 \
  --target-parallel-transfers 4 \
  --target-skip-delete=false
```

This changes the tenant value and worker count and explicitly disables append mode.

For each source table, pgex chooses exactly one condition, in this order:

1. Its entry in `source.selection.table_filters`.
2. The first matching `source.selection.table_rules` entry, in YAML order.
3. The shared `source.selection.where` condition.
4. No condition, selecting all rows.

Conditions are chosen, not combined. **CLI `--where` replaces only the shared
condition:** table-specific filters and matching column rules still take priority.
The column rule matches an exact, case-sensitive column name. Filters for tables
outside the selected set are rejected.

A table's `target.delete_filters` entry takes priority for deletion. Without one,
its effective source condition is used. `skip_delete` skips deletion, while
`truncate` replaces it with whole-table truncation and takes priority over
`skip_delete`.

Every YAML condition is syntax-checked when the file is loaded, even if a CLI
setting will replace it. Parameter values are checked for all conditions retained
after merging, including rules that might not match a table in this run.

## Refresh behavior

### Snapshots and locks

The default `shared` snapshot reads all source tables from one point in time.
With `per-statement`, each source COPY sees its own snapshot; different tables can
reflect different commits on a changing source. Both modes hold source table read
locks until the run ends. Normal source writes can continue; the locks prevent
conflicting schema changes.

Destination table read locks and layout validation are enabled by default. Use
`--no-target-locks` to skip those destination locks; it does not disable layout
validation or source locks. Avoid schema changes during an unlocked transfer and
concurrent writes to destination rows being refreshed. Layout validation can be
disabled separately with `--target-validate-layouts false`.

### Transactions and parallelism

Pgex copies whole tables in parallel, using 8 workers by default. Concurrency is
capped by the source reader limit, also 8 by default. Source table locks and,
when enabled, destination table locks each use an additional connection.

Each worker groups tables into a transaction. It keeps adding tables while its
actual copied payload is below 32 MiB, then commits before taking another table.
A whole table can take a transaction beyond that threshold. Change the threshold
with `--target-commit-group-mib`.

A failed transaction rolls back that group's deletes and copies. Earlier committed
groups remain applied; the entire run is not a single destination transaction.
Loss of the source lock/snapshot session stops the workers and rolls back their
unfinished destination transactions.

### Append and truncate

`--target-skip-delete` appends the selected source rows without removing existing
destination rows. Inserts still obey constraints, so duplicate keys can fail the
transaction. This is an append operation, not an upsert.

`--target-truncate` runs `TRUNCATE ... CASCADE` before each table is copied. It
requires TRUNCATE privileges, skips the job-wide destination locks, and commits
the truncate separately. COPY normally commits per table; with replica role
enabled, its setup and COPY are committed together per table.

**Truncate removes all destination rows, even with a source WHERE condition.**
CASCADE can also empty referencing tables outside the selection or tables already
copied by another worker. If COPY fails, the truncate stays committed. Use distinct
source and destination tables for this mode.

### Triggers, row security, and timeouts

Destination triggers and foreign-key checks run normally. `--target-replica-role`
uses PostgreSQL replica trigger behavior during copying, skipping ordinary
triggers and their foreign-key checks. It requires permission to set
`session_replication_role`.

`--source-row-security-off` and `--target-row-security-off` ask PostgreSQL to fail
if a statement would be subject to a row-level security policy. They do not grant
permission to bypass a policy. When these flags are false, connection row-security
settings apply normally.

Source disconnect checks run every 1,000 ms during active queries. Set
`--client-connection-check-interval-ms 0` to disable them. Nonzero checks require
server-platform support for connection polling.

All four source transaction timeouts default to 0 (disabled), replacing inherited
role defaults on those sessions. Transaction lifetime limits require PostgreSQL
17+. The source lock/snapshot session stays idle during transfer, so its idle
limit must accommodate that period. Reader limits apply to metadata and transfer
reader transactions. All millisecond settings accept 0–2,147,483,647.

## All options

CLI values below use their YAML equivalent and then the listed default when the
CLI option is absent. Flags with `[=BOOL]` accept a bare flag for `true`.
Use `--target-skip-delete=false`, for example, to explicitly disable a YAML setting.
`--target-validate-layouts` requires a `true` or `false` value, for example
`--target-validate-layouts false`.

### Configuration and connections

| CLI option | YAML setting | Default / purpose |
| --- | --- | --- |
| `--config PATH` | — | Optional YAML configuration file. |
| `--source URL` | `source.connection.url` | Required through CLI or YAML; source connection. |
| `--source-user NAME` | `source.connection.user` | URL username, then OS username. |
| `--source-password PASSWORD` | `source.connection.password` | Source password; see credential precedence above. |
| `--ca PATH` | `source.connection.ca_file` | System trust store; optional source CA file. |
| `--target URL` | `target.connection.url` | Required through CLI or YAML; destination connection. |
| `--target-user NAME` | `target.connection.user` | URL username, then OS username. |
| `--target-password PASSWORD` | `target.connection.password` | Destination password; see credential precedence above. |
| `--target-ca PATH` | `target.connection.ca_file` | System trust store; optional destination CA file. |
| `-h`, `--help` | — | Print CLI help and exit. |

### Tables and parameters

| CLI option | YAML setting | Default / purpose |
| --- | --- | --- |
| `--tables TABLE_LIST` | `source.selection.tables` | Discover source tables; explicit input is one comma-separated list with no spaces around commas. |
| `--where PREDICATE` | `source.selection.where` | No shared condition; supply a SQL expression without `WHERE`. |
| `--param NAME=VALUE` | `params.NAME` | No parameter values; repeat for different names. |

### Source sessions and snapshots

| CLI option | YAML setting | Default / purpose |
| --- | --- | --- |
| `--source-snapshot MODE` | `source.snapshot.mode` | `shared`; accepts `shared` or `per-statement`. |
| `--source-reader-limit N` | `source.snapshot.reader_connection_limit` | `8`; positive maximum number of concurrent source readers. |
| `--source-row-security-off[=BOOL]` | `source.session.row_security_off` | `false`; fail statements subject to row security. |
| `--client-connection-check-interval-ms N` | `source.snapshot.client_connection_check_interval_ms` | `1000`; disconnect-check interval, 0 disables it. |
| `--source-exporter-transaction-timeout-ms N` | `source.snapshot.exporter_timeouts.transaction_ms` | `0`; source lock/snapshot transaction lifetime. Requires PostgreSQL 17+ when nonzero. |
| `--source-exporter-idle-in-transaction-timeout-ms N` | `source.snapshot.exporter_timeouts.idle_in_transaction_ms` | `0`; source lock/snapshot session's idle time inside its transaction. |
| `--source-reader-transaction-timeout-ms N` | `source.snapshot.reader_timeouts.transaction_ms` | `0`; each source reader transaction's lifetime. Requires PostgreSQL 17+ when nonzero. |
| `--source-reader-idle-in-transaction-timeout-ms N` | `source.snapshot.reader_timeouts.idle_in_transaction_ms` | `0`; each source reader's idle time inside its transaction. |

### Destination sessions and transfers

| CLI option | YAML setting | Default / purpose |
| --- | --- | --- |
| `--target-parallel-transfers N` | `target.transfer.workers` | `8`; positive worker count, capped by the source reader limit. |
| `--target-commit-group-mib N` | `target.transfer.commit_group_mib` | `32`; positive copied-payload threshold in MiB per worker transaction. |
| `--target-validate-layouts BOOL` | `target.transfer.validate_layouts` | `true`; compare source and destination layouts before transfer. |
| `--no-target-locks[=BOOL]` | `target.transfer.no_locks` | `false`; skip job-wide destination table locks. |
| `--target-skip-delete[=BOOL]` | `target.transfer.skip_delete` | `false`; append rows without a preceding DELETE. |
| `--target-truncate[=BOOL]` | `target.transfer.truncate` | `false`; truncate and commit each destination table before copying. |
| `--target-replica-role[=BOOL]` | `target.session.replica_role` | `false`; use replica trigger behavior for destination copying. |
| `--target-row-security-off[=BOOL]` | `target.session.row_security_off` | `false`; fail statements subject to row security. |

### YAML-only filters

| YAML setting | Default / purpose |
| --- | --- |
| `source.selection.table_rules` | Empty list. Rules are evaluated in YAML order. |
| `source.selection.table_rules[].has_column` | Required for each rule; exact column name to match. |
| `source.selection.table_rules[].where` | Required for each rule; condition for matching tables. |
| `source.selection.table_filters` | Empty map keyed by `schema.table`. |
| `source.selection.table_filters.<schema.table>.where` | Required for each entry; table-specific source condition. |
| `target.delete_filters` | Empty map keyed by `schema.table`. |
| `target.delete_filters.<schema.table>` | Required for each entry; destination deletion condition, written directly as a string. |

The top-level YAML sections are `source`, `target`, and `params`. All fields and
sections are optional except that both connection URLs must be supplied through
CLI or YAML, and rule/filter entries require their fields as listed above.

## Building and contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for source builds, development setup, testing,
and release packaging.

## License

Pgex is licensed under [MIT](LICENSE). Included dependencies retain their own
licenses; see [third-party license notices](licenses/).

# PostgreSQL 18 schema planner

The [upstream directory](native/upstream) is a byte-for-byte copy of PostgreSQL's
`src/bin/pg_dump`, `src/common/fe_memutils.c`, and `COPYRIGHT` at `REL_18_7`,
commit `f5c41f6681c910b3f69951bf6fb97c6d74605d25`. The pgex adapter lives in
[planner](planner).

The [Rust entry point](src/lib.rs) takes a live libpq connection and an exported
snapshot ID. The [C adapter](planner/child_plan.c) calls pg_dump's
`setup_connection()`, discovers objects and takes table locks, sorts and repairs
the dependency graph, and emits archive entries. After
`BuildArchiveDependencies()`, the adapter passes ordered pre-data and post-data
entries, their prerequisites, and source table layouts to Rust. Rust copies
that data while the catalog structs stay in C.

Call the native planner once in a disposable process, as documented on
`child_plan()`. Upstream can exit on errors. On success it leaves the read
transaction open so its source table locks can still protect COPY.

## Build and test

Building the crate requires Git, a C toolchain, Make, Bison, Flex, OpenSSL
headers, and libpq. Non-Apple targets also use `objcopy`. Cargo fetches the
pinned PostgreSQL revision, verifies the vendored files against it, and builds
the frontend libraries and stock reference executable. The default caches are
`target/pg-dump-source` and `target/pg-dump-build`. Use `PG_DUMP_SOURCE` for an
existing checkout at the pinned revision or `PG_DUMP_BUILD` for a separate
native build directory.

```sh
cargo build --locked -p pg-dump-planner
cargo test --locked -p pg-dump-planner
```

The crate tests cover selecting schema prerequisites through cyclic, duplicate,
and data-phase dependencies.

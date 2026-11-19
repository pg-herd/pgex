use std::path::PathBuf;

pub(super) fn pgex_binary() -> PathBuf {
    std::env::var_os("PGEX_TEST_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_pgex")))
}

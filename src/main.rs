mod cli;
mod job_runner;

use std::env;

use anyhow::{Context, Result};
use clap::Parser;
use tokio::io::{self, AsyncWriteExt};

use cli::{Cli, resolve_password};
use job_runner::JobRunner;
use pgex::{
    catalog::CatalogReader,
    config::{
        compiled::TableSelection, compiler::compile_config, loader, snapshot_mode::SnapshotMode,
    },
    connection::Connector,
    snapshot::{SnapshotLease, SnapshotOptions, SourceReadMode},
    targets::{PostgresTarget, PostgresTargetOptions},
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse()
        .into_input()
        .unwrap_or_else(|error| error.exit());
    let file = match cli.config_path {
        Some(path) => Some(loader::load(&path).await?),
        None => None,
    };
    let config = compile_config(cli.input, file)?;
    let tables = match config.source.selection.tables {
        TableSelection::Discover => Vec::new(),
        TableSelection::Explicit(tables) => tables,
    };
    let password = resolve_password(
        config.source.connection.password,
        "PGEX_SOURCE_PASSWORD",
        || env::var("PGEX_SOURCE_PASSWORD"),
    )?;
    let connector = Connector::with_credentials(
        &config.source.connection.url,
        config.source.connection.ca_file.as_deref(),
        config.source.connection.user.as_deref(),
        password.as_deref(),
    )
    .await?;
    let catalog = CatalogReader::new(connector.clone());
    let target_password = resolve_password(
        config.target.connection.password,
        "PGEX_TARGET_PASSWORD",
        || env::var("PGEX_TARGET_PASSWORD"),
    )?;
    let target = Connector::with_credentials(
        &config.target.connection.url,
        config.target.connection.ca_file.as_deref(),
        config.target.connection.user.as_deref(),
        target_password.as_deref(),
    )
    .await
    .context("Configure destination connection")?;
    let target = PostgresTarget::new(
        target,
        PostgresTargetOptions {
            validate_layouts: config.target.transfer.validate_layouts,
            no_locks: config.target.transfer.no_locks,
            workers: config.target.transfer.workers,
            commit_group_mib: config.target.transfer.commit_group_mib,
            replica_role: config.target.session.replica_role,
            row_security_off: config.target.session.row_security_off,
            skip_delete: config.target.transfer.skip_delete,
            truncate: config.target.transfer.truncate,
            delete_filters: config.target.delete_filters,
        },
    );
    let read_mode = match config.source.snapshot.mode {
        SnapshotMode::Shared => SourceReadMode::SharedSnapshot,
        SnapshotMode::PerStatement => SourceReadMode::ReadCommitted,
    };
    let runner = JobRunner::new(catalog, SnapshotLease::new(connector), target);
    runner
        .run(
            &tables,
            SnapshotOptions {
                client_connection_check_interval_ms: config
                    .source
                    .snapshot
                    .client_connection_check_interval_ms,
                reader_connection_limit: config.source.snapshot.reader_connection_limit,
                row_security_off: config.source.session.row_security_off,
                exporter_timeouts: config.source.snapshot.exporter_timeouts,
                reader_timeouts: config.source.snapshot.reader_timeouts,
            },
            read_mode,
            &config.source.selection.filters,
        )
        .await?;
    let mut stdout = io::stdout();
    stdout.write_all(b"Sync complete.\n").await?;
    stdout.flush().await?;
    Ok(())
}

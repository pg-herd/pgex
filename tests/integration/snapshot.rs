mod configuration;
mod locking;
mod reader_limit;
mod session_cleanup;
mod visibility;

use anyhow::Result;
use pgex::connection::Client;

use pgex::snapshot::SnapshotLease;

use super::postgres::PostgresTestDatabase;

async fn with_snapshot_database(
    test: impl AsyncFnOnce(&SnapshotLease, &Client) -> Result<()>,
) -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let connector = database.connector().await?;
    let observer = connector.connect().await?;
    let snapshots = SnapshotLease::new(connector);
    let result = test(&snapshots, observer.client()).await;
    observer.close().await;
    result
}

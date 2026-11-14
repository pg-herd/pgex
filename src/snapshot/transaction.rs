use anyhow::{Context, Result};
use tokio_postgres::types::Type;

use crate::connection::Session;

#[derive(Clone, Copy, Debug, Default)]
pub struct SnapshotTimeouts {
    /// Maximum transaction lifetime in milliseconds (PostgreSQL 17+).
    /// Zero disables the timeout.
    pub transaction_ms: u32,
    /// Maximum idle time between commands in the transaction, in milliseconds.
    /// Zero disables the timeout.
    pub idle_in_transaction_ms: u32,
}

pub(super) async fn begin_read_transaction(
    session: &Session,
    client_connection_check_interval_ms: u32,
    timeouts: SnapshotTimeouts,
) -> Result<()> {
    let interval_ms = i32::try_from(client_connection_check_interval_ms)
        .context("client_connection_check_interval must fit in a PostgreSQL integer")?;
    // Set these on the dedicated snapshot connection before BEGIN. PostgreSQL
    // may keep an already running transaction timer when its value changes.
    configure_timeouts(session, timeouts).await?;
    // SET LOCAL can run before snapshot import without acquiring a snapshot.
    let client = session.client();
    let begun = client
        .batch_execute(&format!(
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; \
             SET LOCAL client_connection_check_interval = {interval_ms}"
        ))
        .await
        .context("Begin read-only snapshot transaction and set client_connection_check_interval");
    if let Err(error) = begun {
        return match client.batch_execute("ROLLBACK").await {
            Ok(()) => Err(error),
            Err(cleanup) => Err(error.context(format!("Rollback also failed: {cleanup:#}"))),
        };
    }
    Ok(())
}

async fn configure_timeouts(session: &Session, timeouts: SnapshotTimeouts) -> Result<()> {
    let transaction_ms = i32::try_from(timeouts.transaction_ms)
        .context("transaction_timeout must fit in a PostgreSQL integer")?;
    let idle_ms = i32::try_from(timeouts.idle_in_transaction_ms)
        .context("idle_in_transaction_session_timeout must fit in a PostgreSQL integer")?;
    // PostgreSQL versions before 17 only support the idle timeout.
    session
        .client()
        .query_typed_one(
            "SELECT CASE WHEN $1 <> 0 OR pg_catalog.current_setting('transaction_timeout', true) IS NOT NULL
                    THEN pg_catalog.set_config('transaction_timeout', $1::pg_catalog.int4::pg_catalog.text, false)
                    END,
                    pg_catalog.set_config('idle_in_transaction_session_timeout', $2::pg_catalog.int4::pg_catalog.text, false)",
            &[(&transaction_ms, Type::INT4), (&idle_ms, Type::INT4)],
        )
        .await
        .context("Set source snapshot transaction timeouts (a nonzero transaction timeout requires PostgreSQL 17+)" )?;
    Ok(())
}

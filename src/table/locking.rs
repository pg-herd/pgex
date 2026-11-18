use anyhow::Result;

use crate::connection::{Connector, Session};

use super::QualifiedTable;

/// Holds read locks on a dedicated session until the callback returns.
pub async fn with_locked_tables<T>(
    connector: &Connector,
    tables: &[QualifiedTable],
    work: impl AsyncFnOnce(&Session) -> Result<T>,
) -> Result<T> {
    connector
        .with_connection(async |session| {
            session
                .with_transaction(async |session| {
                    lock_for_read(session, tables).await?;
                    work(session).await
                })
                .await
        })
        .await
}

/// Acquires ACCESS SHARE locks on a session with an active transaction.
pub async fn lock_for_read(session: &Session, tables: &[QualifiedTable]) -> Result<()> {
    if tables.is_empty() {
        return Ok(());
    }

    let names = {
        let mut guard = session.client().connection().await?;
        let connection = guard.get().await?;
        tables
            .iter()
            .map(|table| connection.quote_qualified_identifier(table.schema(), table.table()))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    };
    session
        .client()
        .batch_execute(&format!("LOCK TABLE {names} IN ACCESS SHARE MODE"))
        .await
        .map_err(|error| describe_lock_failure(tables, error))
}

fn describe_lock_failure(
    tables: &[QualifiedTable],
    error: crate::connection::Error,
) -> anyhow::Error {
    if let Some(database_error) = error.as_database_error()
        && let Some(table) = missing_table(
            tables,
            database_error.code().as_deref().unwrap_or(""),
            database_error.message(),
        )
    {
        return anyhow::Error::new(error).context(format!("Table {} does not exist", table));
    }
    anyhow::Error::new(error).context("Lock tables for read")
}

fn missing_table<'a>(
    tables: &'a [QualifiedTable],
    code: &str,
    message: &str,
) -> Option<&'a QualifiedTable> {
    // These PostgreSQL lookup errors put the object name in the message.
    match code {
        "42P01" => {
            let mut matches = tables.iter().filter(|table| {
                message
                    == format!(
                        "relation \"{}.{}\" does not exist",
                        table.schema(),
                        table.table()
                    )
            });
            let first = matches.next()?;
            // A dot inside an identifier can make two qualified names look identical.
            (!matches.any(|table| table != first)).then_some(first)
        }
        "3F000" => tables
            .iter()
            .find(|table| message == format!("schema \"{}\" does not exist", table.schema())),
        _ => None,
    }
}

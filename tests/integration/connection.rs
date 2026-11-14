use std::future::pending;

use anyhow::{Result, bail};
use tokio::sync::oneshot;

use super::postgres::PostgresTestDatabase;

#[tokio::test]
async fn reuses_one_connection_after_commit_and_rollback() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let connector = database.connector().await?;

    connector
        .with_connection(async |connection| {
            let original_pid: i32 = connection
                .with_transaction(async |session| {
                    session
                        .client().batch_execute(
                            "CREATE TEMP TABLE transaction_probe (id integer);
                             INSERT INTO transaction_probe VALUES (1);",
                        )
                        .await?;
                    Ok(session.client().query_one("SELECT pg_backend_pid()", &[]).await?.try_get(0)?)
                })
                .await?;

            let failure: Result<()> = connection
                .with_transaction(async |session| {
                    session
                        .client().batch_execute("INSERT INTO transaction_probe VALUES (2)")
                        .await?;
                    bail!("abort this transaction")
                })
                .await;
            assert_eq!(failure.unwrap_err().to_string(), "abort this transaction");

            connection
                .with_transaction(async |session| {
                    let row = session
                        .client().query_one(
                            "SELECT pg_backend_pid(), array_agg(id ORDER BY id) FROM transaction_probe",
                            &[],
                        )
                        .await?;
                    assert_eq!(row.try_get::<_, i32>(0)?, original_pid);
                    assert_eq!(row.try_get::<_, Vec<i32>>(1)?, [1]);
                    session
                        .client().batch_execute("INSERT INTO transaction_probe VALUES (3)")
                        .await?;
                    Ok(())
                })
                .await?;

            let row = connection
                .client()
                .query_one("SELECT array_agg(id ORDER BY id) FROM transaction_probe", &[])
                .await?;
            assert_eq!(row.try_get::<_, Vec<i32>>(0)?, [1, 3]);
            Ok(())
        })
        .await
}

#[tokio::test]
async fn cancelling_transaction_work_closes_the_session() -> Result<()> {
    let database = PostgresTestDatabase::start().await?;
    let connector = database.connector().await?;

    connector
        .with_connection(async |session| {
            let (started, ready) = oneshot::channel();
            let mut work = Box::pin(session.with_transaction(async move |session| {
                session.client().query_one("SELECT 1", &[]).await?;
                started.send(()).unwrap();
                pending::<Result<()>>().await
            }));

            tokio::select! {
                result = &mut work => bail!("transaction ended before cancellation: {result:?}"),
                ready = ready => ready?,
            }
            drop(work);

            assert!(session.client().query_one("SELECT 1", &[]).await.is_err());
            Ok(())
        })
        .await
}

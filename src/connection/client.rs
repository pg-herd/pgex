use super::{Error, PgConnection, QueryParameter, RowData};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Mutex, MutexGuard};

pub struct Client {
    connection: Mutex<Option<PgConnection>>,
    closed: AtomicBool,
}
impl Client {
    pub(super) fn new(connection: PgConnection) -> Self {
        Self {
            connection: Mutex::new(Some(connection)),
            closed: AtomicBool::new(false),
        }
    }
    pub async fn connection(&self) -> Result<ConnectionGuard<'_>, Error> {
        let connection = self.connection.lock().await;
        if self.closed.load(Ordering::Acquire) || connection.is_none() {
            return Err(Error::local("Connection closed"));
        }
        Ok(ConnectionGuard { connection })
    }
    pub async fn batch_execute(&self, sql: &str) -> Result<(), Error> {
        self.connection()
            .await?
            .get()
            .await?
            .query(sql, None)
            .await?;
        Ok(())
    }
    pub async fn query(
        &self,
        sql: &str,
        parameters: &[&(dyn QueryParameter + Sync)],
    ) -> Result<Vec<RowData>, Error> {
        Ok(self
            .connection()
            .await?
            .get()
            .await?
            .query(sql, Some(parameters))
            .await?
            .0)
    }
    pub async fn query_one(
        &self,
        sql: &str,
        parameters: &[&(dyn QueryParameter + Sync)],
    ) -> Result<RowData, Error> {
        let mut rows = self.query(sql, parameters).await?;
        if rows.len() != 1 {
            return Err(Error::local(format!(
                "Expected one row, received {}",
                rows.len()
            )));
        }
        Ok(rows.remove(0))
    }
    pub async fn execute(
        &self,
        sql: &str,
        parameters: &[&(dyn QueryParameter + Sync)],
    ) -> Result<u64, Error> {
        Ok(self
            .connection()
            .await?
            .get()
            .await?
            .query(sql, Some(parameters))
            .await?
            .1)
    }
    pub(crate) async fn wait_for_disconnect(&self) -> anyhow::Error {
        let mut connection = self.connection.lock().await;
        match connection.as_mut() {
            Some(connection) => connection.wait_for_disconnect().await.into(),
            None => anyhow::anyhow!("PostgreSQL exporter connection closed"),
        }
    }
    pub(super) fn abort(&self) {
        self.closed.store(true, Ordering::Release);
        if let Ok(mut connection) = self.connection.try_lock() {
            drop(connection.take());
        }
    }
    pub(super) async fn close(&self) {
        self.closed.store(true, Ordering::Release);
        drop(self.connection.lock().await.take());
    }
}
pub struct ConnectionGuard<'a> {
    connection: MutexGuard<'a, Option<PgConnection>>,
}
impl ConnectionGuard<'_> {
    pub async fn get(&mut self) -> Result<&mut PgConnection, Error> {
        self.connection
            .as_mut()
            .ok_or_else(|| Error::local("Connection closed"))
    }
}

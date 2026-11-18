use std::path::Path;

use anyhow::{Context, Result};

use super::{
    PgConnection, Session,
    options::{self, ConnectionOptions},
};

#[derive(Clone)]
pub struct Connector {
    options: ConnectionOptions,
}

impl Connector {
    pub async fn new(
        connection_string: &str,
        ca: Option<&Path>,
        password: Option<&str>,
    ) -> Result<Self> {
        Self::with_credentials(connection_string, ca, None, password).await
    }

    pub async fn with_credentials(
        connection_string: &str,
        ca: Option<&Path>,
        user: Option<&str>,
        password: Option<&str>,
    ) -> Result<Self> {
        Ok(Self {
            options: options::resolve(connection_string, ca, user, password).await?,
        })
    }

    pub async fn connect(&self) -> Result<Session> {
        let connection = tokio::time::timeout(
            self.options.timeout,
            PgConnection::connect(&self.options.conninfo),
        )
        .await
        .context("PostgreSQL connection timed out")?
        .context("Connect to PostgreSQL over verified TLS")?;
        Ok(Session::new(connection))
    }

    pub async fn with_connection<T>(
        &self,
        work: impl AsyncFnOnce(&mut Session) -> Result<T>,
    ) -> Result<T> {
        let mut session = self.connect().await?;
        let result = work(&mut session).await;
        session.close().await;
        result
    }
}

#[cfg(test)]
mod tests;

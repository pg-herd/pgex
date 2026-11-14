use std::path::Path;

use anyhow::{Context, Result};
use openssl::ssl::{SslConnector, SslMethod};
use postgres_openssl::MakeTlsConnector;
use tokio::task::{self, AbortHandle, JoinHandle};
use tokio_postgres::{Client, Config, config::SslMode};

#[derive(Clone)]
pub struct Connector {
    config: Config,
    tls: MakeTlsConnector,
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
        let mut config = connection_string
            .parse::<Config>()
            .map_err(|_| anyhow::anyhow!("Invalid PostgreSQL connection string"))?;
        if let Some(user) = user {
            config.user(user);
        }
        if let Some(password) = password {
            config.password(password);
        }
        config.ssl_mode(SslMode::Require);

        let ca = ca.map(Path::to_path_buf);
        let tls = task::spawn_blocking(move || {
            let mut tls = SslConnector::builder(SslMethod::tls()).context("Configure TLS")?;
            if let Some(ca) = ca {
                tls.set_ca_file(ca).context("Load TLS CA file")?;
            }
            Ok::<_, anyhow::Error>(MakeTlsConnector::new(tls.build()))
        })
        .await
        .context("Join TLS configuration task")??;

        Ok(Self { config, tls })
    }

    pub async fn connect(&self) -> Result<Session> {
        let (client, connection) = self
            .config
            .connect(self.tls.clone())
            .await
            .context("Connect to PostgreSQL over verified TLS")?;
        Ok(Session {
            client,
            driver: tokio::spawn(connection),
        })
    }

    /// Runs work on a new connection and closes it when the work returns.
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

/// Owns the query client and the task that drives the network connection.
pub struct Session {
    client: Client,
    driver: JoinHandle<Result<(), tokio_postgres::Error>>,
}

impl Session {
    pub fn client(&self) -> &Client {
        &self.client
    }

    pub(super) fn abort_handle(&self) -> AbortHandle {
        self.driver.abort_handle()
    }

    /// Observes the end of the driver task. Only await to completion once.
    pub(crate) async fn wait_for_disconnect(&mut self) -> anyhow::Error {
        match (&mut self.driver).await {
            Ok(Err(error)) => error.into(),
            Ok(Ok(())) => anyhow::anyhow!("PostgreSQL connection closed"),
            Err(error) => error.into(),
        }
    }

    pub async fn close(mut self) {
        self.driver.abort();
        let _ = (&mut self.driver).await;
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.driver.abort();
    }
}

#[cfg(test)]
mod tests;

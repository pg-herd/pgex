use anyhow::{Context, Result};
use tokio::task::AbortHandle;

use super::Session;

impl Session {
    /// Runs work in a transaction on this session, committing on success.
    /// The callback leaves transaction control to this method.
    /// Dropping the future before cleanup completes closes the session.
    pub async fn with_transaction<T>(
        &mut self,
        work: impl AsyncFnOnce(&Session) -> Result<T>,
    ) -> Result<T> {
        let mut close_on_drop = CloseSessionOnDrop(Some(self.abort_handle()));
        let client = self.client();
        client
            .batch_execute("BEGIN")
            .await
            .context("Begin transaction")?;

        let result = match work(self).await {
            Ok(value) => {
                client
                    .batch_execute("COMMIT")
                    .await
                    .context("Commit transaction")?;
                Ok(value)
            }
            Err(error) => {
                if let Err(rollback) = client.batch_execute("ROLLBACK").await {
                    return Err(error.context(format!("Rollback also failed: {rollback}")));
                }
                Err(error)
            }
        };
        close_on_drop.0 = None;
        result
    }
}

struct CloseSessionOnDrop(Option<AbortHandle>);

impl Drop for CloseSessionOnDrop {
    fn drop(&mut self) {
        if let Some(driver) = &self.0 {
            driver.abort();
        }
    }
}

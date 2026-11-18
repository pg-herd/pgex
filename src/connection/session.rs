use std::sync::Arc;

use super::{Client, PgConnection};

/// Owns one database connection. Dropping it closes unfinished transactions.
pub struct Session {
    pub(super) client: Arc<Client>,
}

impl Session {
    pub(super) fn new(connection: PgConnection) -> Self {
        Self {
            client: Arc::new(Client::new(connection)),
        }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub async fn close(self) {
        self.client.close().await;
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.client.abort();
    }
}

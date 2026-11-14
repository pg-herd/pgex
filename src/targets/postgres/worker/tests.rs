use std::{future::pending, time::Duration};

use anyhow::{Result, bail};
use tokio::{sync::Semaphore, time::timeout};

use super::run_workers;

#[tokio::test]
async fn later_worker_failure_cancels_the_pending_first_worker() -> Result<()> {
    let worker_started = Semaphore::new(0);
    let session_slot = Semaphore::new(1);

    let result = timeout(
        Duration::from_secs(2),
        run_workers(2, async |worker_id| {
            if worker_id == 1 {
                let _permit = session_slot.acquire().await?;
                worker_started.add_permits(1);
                return pending().await;
            }

            let _started = worker_started.acquire().await?;
            bail!("second worker failed");
        }),
    )
    .await?;

    assert_eq!(result.unwrap_err().to_string(), "second worker failed");
    assert_eq!(session_slot.available_permits(), 1);
    Ok(())
}

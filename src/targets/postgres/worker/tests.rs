use std::{
    collections::HashSet,
    future::pending,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use anyhow::{Result, bail};
use tokio::{
    sync::{Semaphore, watch},
    time::timeout,
};

use super::run_workers;

#[tokio::test]
async fn later_worker_failure_cancels_the_pending_first_worker() -> Result<()> {
    let worker_started = Arc::new(Semaphore::new(0));
    let session_slot = Arc::new(Semaphore::new(1));
    let (_exporter, disconnected) = watch::channel(());

    let result = timeout(
        Duration::from_secs(2),
        run_workers(2, disconnected, |worker_id| {
            let worker_started = Arc::clone(&worker_started);
            let session_slot = Arc::clone(&session_slot);
            async move {
                if worker_id == 1 {
                    let _permit = session_slot.acquire().await?;
                    worker_started.add_permits(1);
                    return pending().await;
                }

                let _started = worker_started.acquire().await?;
                bail!("second worker failed");
            }
        }),
    )
    .await?;

    assert_eq!(result.unwrap_err().to_string(), "second worker failed");
    assert_eq!(session_slot.available_permits(), 1);
    Ok(())
}

#[tokio::test]
async fn each_transfer_worker_stays_on_its_own_thread() -> Result<()> {
    let (_exporter, disconnected) = watch::channel(());
    let supervisor = thread::current().id();
    let threads = Arc::new(Mutex::new(HashSet::new()));

    run_workers(3, disconnected, |_| {
        let threads = Arc::clone(&threads);
        async move {
            let worker = thread::current().id();
            assert_ne!(worker, supervisor);
            tokio::task::yield_now().await;
            assert_eq!(thread::current().id(), worker);
            assert!(threads.lock().unwrap().insert(worker));
            Ok(())
        }
    })
    .await?;

    assert_eq!(threads.lock().unwrap().len(), 3);
    Ok(())
}

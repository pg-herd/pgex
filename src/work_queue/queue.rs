use std::collections::VecDeque;

use tokio::sync::Mutex;

/// A shared FIFO queue seeded with the work for one job.
pub struct WorkQueue<T> {
    items: Mutex<VecDeque<T>>,
}

impl<T> WorkQueue<T> {
    pub fn new(items: impl IntoIterator<Item = T>) -> Self {
        Self {
            items: Mutex::new(items.into_iter().collect()),
        }
    }

    /// Removes the oldest item, or returns `None` when the queue is empty.
    pub async fn pop_front(&self) -> Option<T> {
        self.items.lock().await.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::WorkQueue;

    #[tokio::test]
    async fn hands_out_items_in_input_order_until_empty() {
        let queue = WorkQueue::new([3, 1, 2]);

        assert_eq!(queue.pop_front().await, Some(3));
        assert_eq!(queue.pop_front().await, Some(1));
        assert_eq!(queue.pop_front().await, Some(2));
        assert_eq!(queue.pop_front().await, None);
    }

    #[tokio::test]
    async fn only_one_consumer_receives_the_last_item() {
        let queue = WorkQueue::new([42]);

        let (first, second) = tokio::join!(queue.pop_front(), queue.pop_front());

        let claimed: Vec<_> = [first, second].into_iter().flatten().collect();
        assert_eq!(claimed, [42]);
    }
}

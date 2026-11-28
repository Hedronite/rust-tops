//! RS-06 detached tasks, and RS-28 blocking work on the async executor.

use tokio::task::{JoinError, JoinSet};

/// Owns the background work it starts.
///
/// [`shutdown`](Self::shutdown) waits for every task. Dropping the pool aborts
/// tasks that have not started; a blocking task that has already started runs
/// to completion, because blocking work cannot be interrupted.
#[derive(Debug, Default)]
pub struct TaskPool {
    tasks: JoinSet<u64>,
}

impl TaskPool {
    /// Checksums `data` on the blocking pool, off the async worker threads.
    pub fn checksum_in_background(&mut self, data: Vec<u8>) {
        self.tasks.spawn_blocking(move || checksum(&data));
    }

    /// Waits for every task and returns the results in completion order.
    ///
    /// # Errors
    /// The first [`JoinError`]: a task panicked or was cancelled.
    pub async fn shutdown(mut self) -> Result<Vec<u64>, JoinError> {
        let mut done = Vec::with_capacity(self.tasks.len());
        while let Some(result) = self.tasks.join_next().await {
            done.push(result?);
        }
        Ok(done)
    }
}

/// FNV-1a, 64-bit. Stands in for CPU-heavy work.
#[must_use]
pub fn checksum(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_matches_the_published_vectors() {
        assert_eq!(checksum(b""), 0xcbf2_9ce4_8422_2325, "FNV-1a offset basis");
        assert_eq!(checksum(b"a"), 0xaf63_dc4c_8601_ec8c, "FNV-1a of \"a\"");
    }

    #[tokio::test]
    async fn shutdown_waits_for_every_task() {
        let mut pool = TaskPool::default();
        pool.checksum_in_background(b"a".to_vec());
        pool.checksum_in_background(Vec::new());
        let mut got = pool.shutdown().await.unwrap();
        got.sort_unstable();
        assert_eq!(
            got,
            vec![0xaf63_dc4c_8601_ec8c, 0xcbf2_9ce4_8422_2325],
            "both results come back"
        );
    }
}

//! RS-04 a lock guard held across `.await`; RS-29 lock scope.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

/// A small in-memory cache shared between tasks.
#[derive(Debug, Default)]
pub struct Cache {
    inner: Mutex<HashMap<String, u64>>,
}

impl Cache {
    /// Returns the cached value for `key`, or fetches it with `fetch` and stores it.
    ///
    /// The lock is taken twice, briefly, and never held across `.await`. The
    /// returned future stays `Send`, and a slow `fetch` blocks no other caller.
    /// Two callers that miss at once may both fetch; the later store wins.
    pub async fn get_or_fetch<F, Fut>(&self, key: &str, fetch: F) -> u64
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = u64>,
    {
        if let Some(hit) = self.lock().get(key).copied() {
            return hit;
        }
        let value = fetch().await;
        self.lock().insert(key.to_owned(), value);
        value
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, u64>> {
        self.inner.lock().unwrap_or_else(|poisoned| {
            // A writer panicked while holding the lock. A cache can be rebuilt,
            // so its contents are dropped rather than trusted, and the poison
            // flag is cleared once they are gone.
            let mut map = poisoned.into_inner();
            map.clear();
            self.inner.clear_poison();
            map
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_future_is_send() {
        fn is_send<T: Send>(_: &T) {}
        let cache = Cache::default();
        let pending = cache.get_or_fetch("k", || async { 1 });
        is_send(&pending);
    }

    #[test]
    fn a_poisoned_cache_starts_over() {
        let cache = std::sync::Arc::new(Cache::default());
        cache.lock().insert("k".to_owned(), 1);
        let writer = std::sync::Arc::clone(&cache);
        let crashed = std::thread::spawn(move || {
            let _guard = writer.inner.lock();
            "not a number".parse::<u8>().unwrap()
        })
        .join();
        assert!(crashed.is_err(), "the writer panicked holding the lock");
        assert!(cache.inner.is_poisoned(), "the lock is poisoned");
        assert_eq!(cache.lock().get("k"), None, "poisoned contents are dropped");
        assert!(!cache.inner.is_poisoned(), "and the poison is cleared");
    }

    #[tokio::test]
    async fn a_second_lookup_hits() {
        let cache = Cache::default();
        let first = cache.get_or_fetch("k", || async { 7 }).await;
        let second = cache.get_or_fetch("k", || async { 99 }).await;
        assert_eq!((first, second), (7, 7), "the stored value is reused");
    }
}

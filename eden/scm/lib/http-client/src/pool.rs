/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::mem;
use std::sync::Arc;

use curl::multi::Multi;
use parking_lot::Mutex;

// Maximum number of handles in the pool. Number chosen arbitrarily.
// Note that this number does not limit the number of handles the pool
// will supply to the user, but instead limits the number of handles
// that are retained once they are returned to the pool.
const MAX_POOL_SIZE: usize = 1024;

/// A pool of libcurl `Multi` handles. Each multi session maintains caches for
/// things like TCP connections, TLS sessions, DNS information, etc. As such,
/// it makes sense to reuse multi sessions when possible to benefit from this
/// caching. Since a multi session can only be used by a single thread at a
/// time, this pool provides a mechanism by which threads can use and return
/// `Multi` handles, allowing them to be used repeatedly.
///
/// The `Pool` maintains a priority queue of `Multi` handles, each ranked by
/// how many times it has been used. The assumption is that a handle that has
/// been used more will have a warmer cache and should therefore be preferred.
/// When a borrowed handle is dropped by the thread using it, it will be
/// automatically returned to the pool with its priority incremented.
///
/// Those familiar with libcurl may be aware of libcurl's "share" interface,
/// which allows multiple curl handles (potentially on different threads) to
/// share caches. Unfortunately, the `curl` crate does not provide safe
/// bindings to the share interface, and implementing them would require
/// a lot of subtle unsafe code. While it would be ideal to use the share
/// interface, for now, juggling multi sessions should suffice.
#[derive(Clone)]
pub(crate) struct Pool {
    inner: Arc<PoolInner>,
}

impl Pool {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(PoolInner::new()),
        }
    }

    pub(crate) fn multi(&self) -> PoolMulti {
        self.inner.clone().pop_or_init()
    }
}

/// A borrowed `Multi` handle that is associated with a particular `Pool`.
pub(crate) struct PoolMulti {
    pool: Arc<PoolInner>,
    entry: Option<PoolEntry>,
    valid: bool,
}

impl PoolMulti {
    pub(crate) fn get(&self) -> &Multi {
        &self.entry.as_ref().unwrap().multi
    }

    pub(crate) fn get_mut(&mut self) -> &mut Multi {
        &mut self.entry.as_mut().unwrap().multi
    }

    /// Release the `PoolMulti` without returning it to the pool.
    pub(crate) fn discard(mut self) {
        self.valid = false;
    }
}

impl Drop for PoolMulti {
    fn drop(&mut self) {
        if self.valid {
            self.pool.push(self.entry.take().unwrap());
        }
    }
}

/// Shared state between a `Pool` and its associated `PoolMulti`s.
struct PoolInner {
    heap: Mutex<BinaryHeap<PoolEntry>>,
}

impl PoolInner {
    fn new() -> Self {
        Self {
            heap: Mutex::new(BinaryHeap::new()),
        }
    }

    /// Pop an existing `Multi` or create a new one if none are available.
    fn pop_or_init(self: Arc<PoolInner>) -> PoolMulti {
        let mut heap = self.heap.lock();
        let entry = heap.pop().unwrap_or_else(PoolEntry::new);
        PoolMulti {
            pool: self.clone(),
            entry: Some(entry),
            valid: true,
        }
    }

    /// Return a `Multi` to the pool.
    fn push(&self, mut entry: PoolEntry) {
        entry.priority += 1;
        let mut heap = self.heap.lock();
        heap.push(entry);
        if heap.len() >= MAX_POOL_SIZE {
            truncate(&mut *heap, MAX_POOL_SIZE);
        }
    }
}

/// Truncate the heap to length n, dropping the items with lowest priority.
///
/// XXX: This is inefficient as it has to allocate a new vector and copy over
/// all the items (and then re-heapify them); it is mostly here just to prevent
/// unbounded memory usage in the case where the user performs a very large
/// number of concurrent requests. (It's hard to imagine this number exceeding
/// the pool size limit, but better to be safe than sorry...)
fn truncate<T: Ord>(heap: &mut BinaryHeap<T>, n: usize) {
    let mut sorted = mem::take(heap).into_sorted_vec();
    if let Some(i) = sorted.len().checked_sub(n) {
        *heap = sorted.drain(i..).collect();
    }
}

/// A `Multi` along with its priority in the pool.
struct PoolEntry {
    multi: Multi,
    priority: usize,
}

impl PoolEntry {
    fn new() -> Self {
        Self {
            multi: Multi::new(),
            priority: 0,
        }
    }
}

impl Ord for PoolEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority.cmp(&other.priority)
    }
}

impl PartialOrd for PoolEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for PoolEntry {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

impl Eq for PoolEntry {}

/// From [libcurl's documentation][1]:
///
/// > You must never share the same handle in multiple threads. You can pass the
/// > handles around among threads, but you must never use a single handle from
/// > more than one thread at any given time.
///
/// `Multi` does not implement `Send` or `Sync` because of the above
/// constraints. In particular, it is not `Sync` because libcurl has no
/// internal synchronization, and it is not `Send` because a single Multi
/// session can only be used to drive transfers from a single thread at
/// any one time.
///
/// In this case, when a `PoolEntry` is taken from the pool, it will only
/// be used by a single thread, and returned only when it is dropped. As
/// such, there is no risk of it being used by multiple theads at the same
/// time, so it is safe to mark it as `Send`.
///
/// [1]: https://curl.haxx.se/libcurl/c/threadsafe.html
unsafe impl Send for PoolEntry {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool() {
        let pool = Pool::new();

        {
            // Get a handle for the first time.
            let multi = pool.multi();
            assert_eq!(multi.entry.as_ref().unwrap().priority, 0);
        }

        {
            // Check that we reused the existing handle.
            let multi = pool.multi();
            assert_eq!(multi.entry.as_ref().unwrap().priority, 1);
        }

        {
            // Get 2 handles. The first should be the existing one,
            // the second should be a new one.
            let multi1 = pool.multi();
            let multi2 = pool.multi();
            assert_eq!(multi1.entry.as_ref().unwrap().priority, 2);
            assert_eq!(multi2.entry.as_ref().unwrap().priority, 0);
        }

        {
            // Get two handles again. This time we should get the
            // two existing handles in order of number of uses.
            let multi1 = pool.multi();
            let multi2 = pool.multi();
            assert_eq!(multi1.entry.as_ref().unwrap().priority, 3);
            assert_eq!(multi2.entry.as_ref().unwrap().priority, 1);
        }
    }

    #[test]
    fn test_truncate() {
        let mut heap = BinaryHeap::from(vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        truncate(&mut heap, 5);
        assert_eq!(heap.into_sorted_vec(), vec![5, 6, 7, 8, 9]);
    }
}

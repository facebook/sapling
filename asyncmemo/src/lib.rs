// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Asynchronous Memoizing Cache
//!
//! This crate implements a cache to memoize results calculated by some async process.
//!
//! The primary access method is `Asyncmemo::get()`. If the result has been previously calculated
//! and cached, then the result is directly returned from the cache.
//!
//! Otherwise an implementation of `Filler` which produces new results. Since the process
//! constructing the result is async, each query to fetch the result will poll the process
//! and will update the cache when it finishes.
//!
//! If a value is requested from the cache while the value is being computed, then that task
//! will be added to a notification list and will be woken when the computation changes state
//! (either completes, fails, or makes progress in its async state machine).
//!
//! The `fill()` function returns an instance of `Future`, and as such can fail. In that case
//! no result will be cached, and the error will be returned to the task that's currently
//! calling that future's `poll()`. Other tasks waiting will be woken, but they'll simply get
//! cache miss and will attempt to compute the result again. There is no negative caching
//! or rate limiting, so if process is prone to failure then it can "succeed" but return a
//! sentinel value representing the failure which the application can handle with its own logic.
//!
//! TODO: add interface to allow multiple implementations of the underlying cache, to allow
//!   eviction and other policies to be controlled.
//!
//! TODO: entry invalidation interface
#![deny(warnings)]

extern crate futures;
extern crate linked_hash_map;
extern crate heapsize;

use std::fmt::{self, Debug};
use std::hash::Hash;
use std::mem;
use std::sync::{Arc, Mutex};
use std::usize;

use futures::{Async, Future, Poll};
use futures::future::IntoFuture;
use futures::task::{self, Task};

#[cfg(test)]
mod test;

mod boundedhash;
mod weight;

use boundedhash::BoundedHash;
pub use weight::Weight;

/// Asynchronous memoizing cache for async processes
///
/// The cache requires an instance of an implementation of the `Filler` trait
/// to generate new results.
pub struct Asyncmemo<F>
where
    F: Filler,
    F::Key: Eq + Hash,
{
    inner: Arc<AsyncmemoInner<F>>,
}

impl<F> Debug for Asyncmemo<F>
where
    F: Filler,
    F::Key: Eq + Hash + Debug,
    <<F as Filler>::Value as IntoFuture>::Future: Debug,
    <<F as Filler>::Value as IntoFuture>::Item: Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Asyncmemo")
            .field("inner", &self.inner)
            .finish()
    }
}

/// Generate a result for the cache.
///
/// The function implemented by `fill()` should be referentially transparent - the output
/// should only depend on the value of the `key` parameter.
/// It may fail - the failure will be propagated to one of the callers, and the result won't
/// be cached.
pub trait Filler: Sized {
    type Key: Eq + Hash;
    type Value: IntoFuture;

    fn fill(&self, cache: &Asyncmemo<Self>, key: &Self::Key) -> Self::Value;
}

type FillerSlot<F> = Slot<<<F as Filler>::Value as IntoFuture>::Future>;

// We really want a type bound on F, but currently that emits an annoying E0122 warning
type CacheHash<F> = BoundedHash<<F as Filler>::Key, FillerSlot<F>>;

struct AsyncmemoInner<F>
where
    F: Filler,
    F::Key: Eq + Hash,
{
    hash: Mutex<CacheHash<F>>,
    filler: F,
}

impl<F> Debug for AsyncmemoInner<F>
where
    F: Filler,
    F::Key: Eq + Hash + Debug,
    <<F as Filler>::Value as IntoFuture>::Future: Debug,
    <<F as Filler>::Value as IntoFuture>::Item: Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let hash = self.hash.lock().expect("poisoned lock");
        fmt.debug_struct("AsyncmemoInner")
            .field("hash", &*hash)
            .finish()
    }
}

#[derive(Debug, Clone)]
enum Slot<F>
where
    F: Future,
{
    Waiting(F),         // waiting for entry to become available
    Polling(Vec<Task>), // Future currently being polled, with waiting Tasks
    Complete(F::Item),  // got value
}

impl<F> Slot<F>
where
    F: Future,
{
    fn is_waiting(&self) -> bool {
        match self {
            &Slot::Waiting(_) => true,
            _ => false,
        }
    }
}

impl<F> Weight for Slot<F>
where
    F: Future,
    F::Item: Weight,
{
    fn get_weight(&self) -> usize {
        match self {
            &Slot::Polling(_) | &Slot::Waiting(_) => 0,
            &Slot::Complete(ref v) => v.get_weight(),
        }
    }
}

/// Pending result from a cache lookup
pub struct MemoFuture<F>
where
    F: Filler,
    F::Key: Eq + Hash,
{
    cache: Asyncmemo<F>,
    key: F::Key,
}

impl<F> Debug for MemoFuture<F>
where
    F: Filler,
    F::Key: Eq + Hash + Debug,
    <<F as Filler>::Value as IntoFuture>::Future: Debug,
    <<F as Filler>::Value as IntoFuture>::Item: Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("MemoFuture")
            .field("cache", &self.cache)
            .field("key", &self.key)
            .finish()
    }
}

fn wake_tasks(tasks: Vec<Task>) {
    for t in tasks {
        t.notify();
    }
}

impl<F> MemoFuture<F>
where
    F: Filler,
    F::Key: Eq + Hash + Weight + Clone,
    <F::Value as IntoFuture>::Item: Weight + Clone,
{
    // Return the current state of a slot, if present
    fn slot_present(&self) -> Option<FillerSlot<F>> {
        let mut hash = self.cache.inner.hash.lock().expect("locked poisoned");

        if let Some(entry) = hash.get_mut(&self.key) {
            match entry {
                // straightforward cache hit
                &mut Slot::Complete(ref val) => return Some(Slot::Complete(val.clone())),

                // Someone else is polling on the future, so just add ourselves to the set of
                // interested tasks and return.
                &mut Slot::Polling(ref mut wait) => {
                    wait.push(task::current());
                    return Some(Slot::Polling(Vec::new()));
                }

                // Last possibility: we're waiting and there's a future
                entry => {
                    let waiting = mem::replace(entry, Slot::Polling(Vec::new()));
                    assert!(waiting.is_waiting());
                    return Some(waiting);
                }
            }
        }

        // There's no existing entry, but we're about to make one so put in a placeholder
        // XXX use entry API?
        let _ = hash.insert(self.key.clone(), Slot::Polling(Vec::new()));
        None
    }

    fn slot_remove(&self) {
        let mut hash = self.cache.inner.hash.lock().expect("locked poisoned");

        if let Some(Slot::Polling(tasks)) = hash.remove(&self.key) {
            wake_tasks(tasks);
        }
    }

    fn slot_insert(&self, slot: FillerSlot<F>) {
        let mut hash = self.cache.inner.hash.lock().expect("locked poisoned");

        match hash.insert(self.key.clone(), slot) {
            Err((_k, _v)) => {
                // failed to insert for capacity reasons; remove entry we're not going to use
                // XXX retry once?
                hash.remove(&self.key);
            }
            Ok(Some(val @ Slot::Complete(_))) => {
                // If we just kicked out a complete value, put it back, since at best
                // we're replacing a complete value with another one (which should be
                // identical), but at worst we could be making it regress. This could only
                // happen if in-progress slot got evicted and the computation restarted.
                let _ = hash.insert(self.key.clone(), val);

                // trim cache if that made it oversized
                hash.trim();
            }
            Ok(Some(Slot::Polling(tasks))) => wake_tasks(tasks),
            Ok(Some(_)) | Ok(None) => (),   // nothing (interesting) there
        }
    }

    fn handle(&self) -> Poll<<Self as Future>::Item, <Self as Future>::Error> {
        // First check to see if we already have a slot for this key and process it accordingly.
        match self.slot_present() {
            None => (),     // nothing there for this key
            Some(Slot::Complete(v)) => return Ok(Async::Ready(v)),
            Some(Slot::Polling(_)) => return Ok(Async::NotReady),
            Some(Slot::Waiting(mut fut)) => match fut.poll() {
                Err(err) => {
                    self.slot_remove();
                    return Err(err);
                }
                Ok(Async::NotReady) => {
                    self.slot_insert(Slot::Waiting(fut));
                    return Ok(Async::NotReady);
                }
                Ok(Async::Ready(val)) => {
                    self.slot_insert(Slot::Complete(val.clone()));
                    return Ok(Async::Ready(val));
                }
            },
        };

        // Slot was not present, but we have a placeholder now. Construct the Future and
        // start running it.

        let mut filler = self.cache
            .inner
            .filler
            .fill(&self.cache, &self.key)
            .into_future();

        match filler.poll() {
            Err(err) => {
                // got an error - remove unused entry and bail
                self.slot_remove();
                return Err(err);
            }
            Ok(Async::NotReady) => {
                self.slot_insert(Slot::Waiting(filler));
                return Ok(Async::NotReady);
            }
            Ok(Async::Ready(val)) => {
                self.slot_insert(Slot::Complete(val.clone()));
                return Ok(Async::Ready(val));
            }
        }
    }
}

impl<F> Future for MemoFuture<F>
where
    F: Filler,
    F::Key: Eq + Hash + Weight + Clone,
    <F::Value as IntoFuture>::Item: Weight + Clone,
{
    type Item = <F::Value as IntoFuture>::Item;
    type Error = <F::Value as IntoFuture>::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.handle()
    }
}

impl<F> Asyncmemo<F>
where
    F: Filler,
    F::Key: Hash + Eq + Weight,
    <F::Value as IntoFuture>::Item: Weight,
{
    /// Construct a new bounded cache. It enforces two distinct limits:
    /// - entrylimit - the max number of entries
    /// - weightlimit - the max abstract "weight" of the entries (both keys and values)
    ///
    /// Weight is typically memory use.
    pub fn with_limits(fill: F, entrylimit: usize, weightlimit: usize) -> Self {
        assert!(entrylimit > 0);
        assert!(weightlimit > 0);

        let inner = AsyncmemoInner {
            hash: Mutex::new(BoundedHash::new(entrylimit, weightlimit)),
            filler: fill,
        };

        Asyncmemo {
            inner: Arc::new(inner),
        }
    }

    /// Construct an unbounded cache.
    ///
    /// This is pretty dangerous for any non-toy use.
    pub fn new_unbounded(fill: F) -> Self {
        Self::with_limits(fill, usize::MAX, usize::MAX)
    }

    /// Look up a result for a particular key/arg
    ///
    /// The future will either complete immediately if the result is already
    /// known, or will wait for an in-progress result (perhaps failing), or
    /// initiate new process to generate a result.
    ///
    /// This only caches successful results - it does not cache errors as a
    /// negative cache.
    pub fn get<K: Into<F::Key>>(&self, key: K) -> MemoFuture<F> {
        MemoFuture {
            cache: self.clone(),
            key: key.into(),
        }
    }

    /// Invalidate a specific key
    pub fn invalidate<K: Into<F::Key>>(&self, key: K) {
        let mut locked = self.inner.hash.lock().expect("lock poison");
        let key = key.into();
        let _ = locked.remove(&key);
    }

    /// Reset the cache. This removes all results (complete and in-progress) from the cache.
    /// This drops the futures of in-progress entries, which should propagate cancellation
    /// if necessary.
    pub fn clear(&self) {
        let mut locked = self.inner.hash.lock().expect("lock poison");

        locked.clear()
    }

    /// Trim cache size to limits.
    pub fn trim(&self) {
        let mut locked = self.inner.hash.lock().expect("lock poison");

        locked.trim_entries(0);
        locked.trim_weight(0);
    }

    /// Return number of entries in cache.
    pub fn len(&self) -> usize {
        let hash = self.inner.hash.lock().expect("lock poison");
        hash.len()
    }

    /// Return true if cache is empty.
    pub fn is_empty(&self) -> bool {
        let hash = self.inner.hash.lock().expect("lock poison");
        hash.is_empty()
    }
}

impl<F> Clone for Asyncmemo<F>
where
    F: Filler,
    F::Key: Eq + Hash,
{
    fn clone(&self) -> Self {
        Asyncmemo {
            inner: self.inner.clone(),
        }
    }
}

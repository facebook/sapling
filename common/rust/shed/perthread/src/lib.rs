/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

//! Provides `ThreadMap` structure for accessing `PerThread` thread local variables from a static
//! context via `ThreadMap::for_each`.
//!
//! Notes:
//! If we wanted to do a global accumulator for the per-thread stats we'd need to:
//!
//! 1. define a counter/stat type. It needs to be Sync to satisfy PerThread/ThreadMap's
//!    constraints.
//! 2. set up a periodic thread/process to enumerate all the stats and accumulate them
//! 3. give the stat type a Drop implementation which also updates the accumulator, so that stats
//!    are recorded when the thread dies (otherwise it loses stats after the last accumulator pass,
//!    and short-lived threads may never record stats at all)
//!
//! Examples:
//! ```
//! use std::sync::LazyLock;
//!
//! use perthread::PerThread;
//! use perthread::ThreadMap;
//!
//! // Set up the map of per-thread counters
//! static COUNTERS: LazyLock<ThreadMap<usize>> = LazyLock::new(ThreadMap::default);
//!
//! // Declare a specific per-thread counter
//! thread_local! {
//!     static COUNTER: PerThread<usize> = COUNTERS.register(0);
//! }
//!
//! COUNTER.with(|c| println!("COUNTER: {:?}", *c));
//! ```

#![deny(warnings, missing_docs, clippy::all, rustdoc::broken_intra_doc_links)]

use std::collections::HashMap;
use std::fmt;
use std::fmt::Debug;
use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::Mutex;

#[derive(Debug, Eq, PartialEq, Hash, Copy, Clone)]
struct Handle(usize);

/// This is a structure that lets you define a map with thread local variables,
/// but also gives you access to all them behind a Mutex.
pub struct ThreadMap<T> {
    inner: Mutex<ThreadMapLocked<T>>,
}

struct ThreadMapLocked<T> {
    idx: usize,
    map: HashMap<Handle, NonNull<T>>,
}

// The raw pointer makes ThreadMapLocked<T> not automatically Send which makes
// Mutex<ThreadMapLocked<T>> neither Send nor Sync.
//
// The mutation of ThreadMapLocked are all synchronized by a mutex so sending or
// sharing the mutex across threads won't cause data races. An empty map is fine
// to send or share regardless of T. A nonempty map can only exist if T: Sync
// because ThreadMap::register enforces that bound. Having T: Sync gives us
// permission to share references to T across threads by either sending or
// sharing the map; after the map is sent or shared the caller may use
// ThreadMap::for_each to access &T from the other thread.
unsafe impl<T> Send for ThreadMap<T> {}
unsafe impl<T> Sync for ThreadMap<T> {}

impl<T> Default for ThreadMap<T> {
    fn default() -> Self {
        Self {
            inner: Mutex::new(ThreadMapLocked {
                idx: 0,
                map: HashMap::new(),
            }),
        }
    }
}

impl<T> ThreadMap<T> {
    /// Register a new per-thread value with the thread map.
    pub fn register(&'static self, val: T) -> PerThread<T>
    where
        T: 'static + Sync,
    {
        // Does not require T: Send because ownership of T remains on its
        // original thread. The caller is free to move ownership of PerThread<T>
        // to a different thread themselves later. But that operation requires
        // PerThread<T>: Send which requires T: Send.

        let mut storage = Box::new(StableStorage {
            val,
            handle: Handle(0), // replaced below after locking map
            map: NonNull::from(self),
        });

        let mut locked = self.inner.lock().expect("poisoned lock");
        storage.handle = Handle(locked.idx);
        locked
            .map
            .insert(storage.handle, NonNull::from(&storage.val));
        locked.idx += 1;

        // Beginning here, PerThread's Drop implementation is in charge of
        // removing the entry from the map.
        //
        // It's possible and legal for the caller to leak their PerThread<T>
        // without dropping it. In that case the inner StableStorage<T> will
        // continue to exist indefinitely which isn't a memory safety violation.
        PerThread { storage }
    }

    /// Enumerate the per-thread values
    ///
    /// This can't be an iterator because we need to control
    /// the lifetime of the returned reference, which is limited to the time
    /// we're holding the mutex (which is what's protecting the value from being
    /// destroyed while we're using it).
    pub fn for_each<F>(&self, mut cb: F)
    where
        // Note that we require the caller's closure to accept references with
        // an arbitrarily short lifetime. The trait bound `FnMut(&T)` is really
        // a higher-rank trait bound equivalent to `for<'r> FnMut(&'r T)`. The
        // values passed to the closure do not necessarily live as long as the
        // map does. In particular, the signature of for_each is *not* the same
        // as `fn for_each<'a, F>(&'a self, cb: F) where F: FnMut(&'a T)`. That
        // signature would be unsound! See comments in D13453346 for an example
        // of safe code triggering use-after-free if that were the signature.
        F: FnMut(&T),
    {
        let locked = self.inner.lock().expect("lock poisoned");

        for val in locked.map.values() {
            cb(unsafe { val.as_ref() });
        }
    }

    fn unregister(&self, h: Handle) {
        let mut locked = self.inner.lock().expect("poisoned lock");
        locked.map.remove(&h);
    }
}

/// Values inserted into the map are returned to the caller inside this wrapper.
/// The caller will hold on to this wrapper as long as they like, then when they
/// drop it the corresponding entry is removed from the map.
///
/// The map data structure holds a pointer `NonNull<T>` to the content of the
/// storage box. We must not expose an API through which the owner of a
/// `PerThread<T>` could invalidate that pointer, for example by moving the content
/// or dropping the content outside of PerThread's Drop impl.
pub struct PerThread<T> {
    storage: Box<StableStorage<T>>,
}

struct StableStorage<T> {
    val: T,
    handle: Handle,

    // Effectively &'static ThreadMap<T>. We enforce in ThreadMap::register that
    // T: 'static. We could use &'static ThreadMap<T> here as the field type but
    // then Rust would require an explicit `T: 'static` on every data structure
    // that transitively contains a generic PerThread<T>. Instead this approach
    // lets us require that bound only for producing any ThreadMap<T> i.e. we
    // know a PerThread<T> can only exist if T: 'static even without spelling
    // out that bound everywhere.
    map: NonNull<ThreadMap<T>>,
}

// Required in order for PerThread<T> to be held in a once_cell in the single
// threaded use case. The main motivating use case for ThreadMap and PerThread
// does not involve sharing PerThread<T> instances across threads, but the
// design is compatible with that and it is safe to do so.
//
// Sending PerThread<T> to another thread requires T: Send. The primary
// situation of a type that is Sync but not Send is when an object needs to be
// destroyed on the same thread that created it, for example because the Drop
// impl accesses thread local storage. For such types it would not be safe to
// send PerThread<T> either.
//
// Sync requires no further bounds because the T is already shared across
// threads -- any thread can obtain a reference to the T through
// ThreadMap::for_each.
unsafe impl<T: Send> Send for StableStorage<T> {}
unsafe impl<T> Sync for StableStorage<T> {}

impl<T> Debug for PerThread<T>
where
    T: Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.storage.val.fmt(fmt)
    }
}

impl<T> Drop for PerThread<T> {
    fn drop(&mut self) {
        let map = unsafe { self.storage.map.as_ref() };
        map.unregister(self.storage.handle)
    }
}

impl<T> AsRef<T> for PerThread<T> {
    fn as_ref(&self) -> &T {
        &self.storage.val
    }
}

// Do not implement AsMut or DerefMut. Must not expose any accessor from
// PerThread<T> to &mut T because some other thread may be iterating inside of
// ThreadMap::for_each while this thread's exclusive reference would exist. An
// exclusive reference here and a shared reference inside of for_each must not
// be possible to exist at the same time.
impl<T> Deref for PerThread<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::hash::Hash;
    use std::sync::LazyLock;

    use super::*;

    fn assert_map_content<T>(map: &ThreadMap<T>, expected: &HashSet<T>)
    where
        T: Clone + fmt::Debug + Hash + Eq,
    {
        let mut set = HashSet::new();
        map.for_each(|el| assert!(set.insert(el.clone())));
        assert_eq!(&set, expected);
    }

    #[test]
    fn test_single_thread() {
        static TEST_MAP: LazyLock<ThreadMap<i64>> = LazyLock::new(ThreadMap::default);
        static TEST_VAL1: LazyLock<PerThread<i64>> = LazyLock::new(|| TEST_MAP.register(42));
        static TEST_VAL2: LazyLock<PerThread<i64>> = LazyLock::new(|| TEST_MAP.register(431));

        let mut expected_values = HashSet::new();
        assert_map_content(&*TEST_MAP, &expected_values);

        assert_eq!(**TEST_VAL1, 42);
        expected_values.insert(**TEST_VAL1);
        assert_map_content(&*TEST_MAP, &expected_values);

        assert_eq!(**TEST_VAL2, 431);
        expected_values.insert(**TEST_VAL2);
        assert_map_content(&*TEST_MAP, &expected_values);
    }

    #[test]
    fn test_integration_with_thread_local() {
        use std::sync::mpsc::sync_channel;
        struct Ack;

        static TEST_MAP: LazyLock<ThreadMap<i64>> = LazyLock::new(ThreadMap::default);

        thread_local! {
            static TEST_VAL1: PerThread<i64> = TEST_MAP.register(7);
            static TEST_VAL2: PerThread<i64> = TEST_MAP.register(42);
        }

        let (sender, receiver) = sync_channel(0);
        let (r_sender, r_receiver) = sync_channel(0);

        let test_thread = ::std::thread::spawn(move || {
            receiver.recv().unwrap();
            TEST_VAL1.with(|val| assert_eq!(**val, 7));
            r_sender.send(Ack).unwrap();

            receiver.recv().unwrap();
            TEST_VAL2.with(|val| assert_eq!(**val, 42));
            r_sender.send(Ack).unwrap();

            receiver.recv().unwrap();
        });

        let mut expected_values = HashSet::new();

        assert_map_content(&*TEST_MAP, &expected_values);

        sender.send(Ack).unwrap();
        r_receiver.recv().unwrap();
        expected_values.insert(7);
        assert_map_content(&*TEST_MAP, &expected_values);

        sender.send(Ack).unwrap();
        r_receiver.recv().unwrap();
        expected_values.insert(42);
        assert_map_content(&*TEST_MAP, &expected_values);

        sender.send(Ack).unwrap();
        test_thread.join().unwrap();
        expected_values.clear();
        assert_map_content(&*TEST_MAP, &expected_values);
    }
}

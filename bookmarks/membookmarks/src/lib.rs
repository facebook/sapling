// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate bookmarks;
#[macro_use]
extern crate error_chain;
extern crate futures;

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Mutex;
use std::sync::atomic::{ATOMIC_USIZE_INIT, AtomicUsize, Ordering};

use futures::future::{FutureResult, ok};
use futures::stream::{BoxStream, Stream, iter};

use bookmarks::{Bookmarks, BookmarksMut, ListBookmarks, Version};

mod errors {
    // Create Error, ErrorKind, ResultExt, and Result types.
    error_chain!{}
}
use errors::*;

static VERSION_COUNTER: AtomicUsize = ATOMIC_USIZE_INIT;

fn version_next() -> Version {
    Version::from(VERSION_COUNTER.fetch_add(1, Ordering::Relaxed) as u64)
}

/// Generic, in-memory bookmark store backed by a HashMap, intended to be used in tests.
pub struct MemBookmarks<V: Clone> {
    bookmarks: Mutex<HashMap<Vec<u8>, (V, Version)>>,
}

impl<V: Clone> MemBookmarks<V> {
    pub fn new() -> Self {
        MemBookmarks { bookmarks: Mutex::new(HashMap::new()) }
    }
}

impl<V> Bookmarks for MemBookmarks<V>
where
    V: Clone + Send + 'static,
{
    type Value = V;
    type Error = Error;

    type Get = FutureResult<Option<(Self::Value, Version)>, Self::Error>;

    fn get(&self, key: &AsRef<[u8]>) -> Self::Get {
        ok(
            self.bookmarks
                .lock()
                .unwrap()
                .get(key.as_ref())
                .map(Clone::clone),
        )
    }
}

impl<V> BookmarksMut for MemBookmarks<V>
where
    V: Clone + Send + 'static,
{
    type Set = FutureResult<Option<Version>, Self::Error>;

    fn set(&self, key: &AsRef<[u8]>, value: &Self::Value, version: &Version) -> Self::Set {
        let mut bookmarks = self.bookmarks.lock().unwrap();

        match bookmarks.entry(key.as_ref().to_vec()) {
            Entry::Occupied(mut entry) => {
                if *version == entry.get().1 {
                    let new = version_next();
                    entry.insert((value.clone(), new));
                    return ok(Some(new));
                } else {
                    ok(None)
                }
            }
            Entry::Vacant(entry) => {
                if *version == Version::absent() {
                    let new = version_next();
                    entry.insert((value.clone(), new));
                    return ok(Some(new));
                } else {
                    ok(None)
                }
            }
        }
    }

    fn delete(&self, key: &AsRef<[u8]>, version: &Version) -> Self::Set {
        let mut bookmarks = self.bookmarks.lock().unwrap();

        match bookmarks.entry(key.as_ref().to_vec()) {
            Entry::Occupied(entry) => {
                if *version == entry.get().1 {
                    entry.remove();
                    ok(Some(Version::absent()))
                } else {
                    ok(None)
                }
            }
            Entry::Vacant(_) => {
                if *version == Version::absent() {
                    ok(Some(Version::absent()))
                } else {
                    ok(None)
                }
            }
        }
    }
}

impl<V> ListBookmarks for MemBookmarks<V>
where
    V: Clone + Send + 'static,
{
    type Keys = BoxStream<Vec<u8>, Self::Error>;

    fn keys(&self) -> Self::Keys {
        let guard = self.bookmarks.lock().unwrap();
        let keys = guard.keys().map(|k| k.clone()).collect::<Vec<_>>();
        iter(keys.into_iter().map(|k| Ok(k))).boxed()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::Future;
    use futures::Stream;

    #[test]
    fn test_single() {
        let bookmarks = MemBookmarks::new();
        assert_eq!(bookmarks.get(&"foo").wait().unwrap(), None);

        let absent = Version::absent();
        let foo_v1 = bookmarks
            .set(&"foo", &"1", &absent)
            .wait()
            .unwrap()
            .unwrap();
        assert_eq!(bookmarks.get(&"foo").wait().unwrap(), Some(("1", foo_v1)));

        let foo_v2 = bookmarks
            .set(&"foo", &"2", &foo_v1)
            .wait()
            .unwrap()
            .unwrap();

        // Should fail due to version mismatch.
        assert_eq!(bookmarks.set(&"foo", &"3", &foo_v1).wait().unwrap(), None);

        assert_eq!(
            bookmarks.delete(&"foo", &foo_v2).wait().unwrap().unwrap(),
            absent
        );
        assert_eq!(bookmarks.get(&"foo").wait().unwrap(), None);

        // Even though bookmark doesn't exist, this should fail with a version mismatch.
        assert_eq!(bookmarks.delete(&"foo", &foo_v2).wait().unwrap(), None);

        // Deleting it with the absent version should work.
        assert_eq!(
            bookmarks.delete(&"foo", &absent).wait().unwrap().unwrap(),
            absent
        );
    }

    #[test]
    fn test_list() {
        let bookmarks = MemBookmarks::new();
        let _ = bookmarks.create(&"A", &"foo").wait().unwrap().unwrap();
        let _ = bookmarks.create(&"B", &"bar").wait().unwrap().unwrap();
        let _ = bookmarks.create(&"C", &"baz").wait().unwrap().unwrap();

        let mut result = bookmarks.keys().collect().wait().unwrap();
        result.sort();

        let expected = vec![b"A", b"B", b"C"];
        assert_eq!(result, expected);
    }
}

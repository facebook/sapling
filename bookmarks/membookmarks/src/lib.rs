// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![feature(never_type)]

extern crate bookmarks;
extern crate futures;
extern crate futures_ext;
extern crate storage_types;

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering, ATOMIC_USIZE_INIT};

use futures::future::{ok, FutureResult};
use futures::stream::iter_ok;

use bookmarks::{Bookmarks, BookmarksMut};
use futures_ext::{BoxStream, StreamExt};
use storage_types::Version;

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
        MemBookmarks {
            bookmarks: Mutex::new(HashMap::new()),
        }
    }
}

impl<V> Bookmarks for MemBookmarks<V>
where
    V: Clone + Send + 'static,
{
    type Value = V;
    type Error = !;

    type Get = FutureResult<Option<(Self::Value, Version)>, Self::Error>;
    type Keys = BoxStream<Vec<u8>, Self::Error>;

    fn get(&self, key: &AsRef<[u8]>) -> Self::Get {
        ok(
            self.bookmarks
                .lock()
                .unwrap()
                .get(key.as_ref())
                .map(Clone::clone),
        )
    }

    fn keys(&self) -> Self::Keys {
        let guard = self.bookmarks.lock().unwrap();
        let keys = guard.keys().map(|k| k.clone()).collect::<Vec<_>>();
        iter_ok(keys.into_iter()).boxify()
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
            Entry::Occupied(mut entry) => if *version == entry.get().1 {
                let new = version_next();
                entry.insert((value.clone(), new));
                return ok(Some(new));
            } else {
                ok(None)
            },
            Entry::Vacant(entry) => if *version == Version::absent() {
                let new = version_next();
                entry.insert((value.clone(), new));
                return ok(Some(new));
            } else {
                ok(None)
            },
        }
    }

    fn delete(&self, key: &AsRef<[u8]>, version: &Version) -> Self::Set {
        let mut bookmarks = self.bookmarks.lock().unwrap();

        match bookmarks.entry(key.as_ref().to_vec()) {
            Entry::Occupied(entry) => if *version == entry.get().1 {
                entry.remove();
                ok(Some(Version::absent()))
            } else {
                ok(None)
            },
            Entry::Vacant(_) => if *version == Version::absent() {
                ok(Some(Version::absent()))
            } else {
                ok(None)
            },
        }
    }
}

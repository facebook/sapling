// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![feature(never_type)]

extern crate bookmarks;
extern crate futures;
extern crate futures_ext;
extern crate mercurial_types;
extern crate storage_types;

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::error;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering, ATOMIC_USIZE_INIT};

use futures::future::ok;
use futures::stream::iter_ok;

use bookmarks::{Bookmarks, BookmarksMut};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use mercurial_types::NodeHash;
use storage_types::Version;

static VERSION_COUNTER: AtomicUsize = ATOMIC_USIZE_INIT;

fn version_next() -> Version {
    Version::from(VERSION_COUNTER.fetch_add(1, Ordering::Relaxed) as u64)
}

/// In-memory bookmark store backed by a HashMap, intended to be used in tests.
pub struct MemBookmarks {
    bookmarks: Mutex<HashMap<Vec<u8>, (NodeHash, Version)>>,
}

impl MemBookmarks {
    pub fn new() -> Self {
        MemBookmarks {
            bookmarks: Mutex::new(HashMap::new()),
        }
    }
}

impl Bookmarks for MemBookmarks {
    fn get(&self, key: &AsRef<[u8]>) -> BoxFuture<Option<(NodeHash, Version)>, bookmarks::Error> {
        ok(
            self.bookmarks
                .lock()
                .unwrap()
                .get(key.as_ref())
                .map(Clone::clone),
        ).boxify()
    }

    fn keys(&self) -> BoxStream<Vec<u8>, bookmarks::Error> {
        let guard = self.bookmarks.lock().unwrap();
        let keys = guard.keys().map(|k| k.clone()).collect::<Vec<_>>();
        iter_ok(keys.into_iter()).boxify()
    }
}

impl BookmarksMut for MemBookmarks {
    fn set(
        &self,
        key: &AsRef<[u8]>,
        value: &NodeHash,
        version: &Version,
    ) -> BoxFuture<Option<Version>, bookmarks::Error> {
        let mut bookmarks = self.bookmarks.lock().unwrap();

        match bookmarks.entry(key.as_ref().to_vec()) {
            Entry::Occupied(mut entry) => if *version == entry.get().1 {
                let new = version_next();
                entry.insert((value.clone(), new));
                ok(Some(new))
            } else {
                ok(None)
            },
            Entry::Vacant(entry) => if *version == Version::absent() {
                let new = version_next();
                entry.insert((value.clone(), new));
                ok(Some(new))
            } else {
                ok(None)
            },
        }.boxify()
    }

    fn delete(
        &self,
        key: &AsRef<[u8]>,
        version: &Version,
    ) -> BoxFuture<Option<Version>, bookmarks::Error> {
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
        }.boxify()
    }
}

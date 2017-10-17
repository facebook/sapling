// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]

extern crate futures;

extern crate futures_ext;
extern crate heads;

use std::hash::Hash;
use std::sync::Mutex;

use futures::future::{ok, FutureResult};
use futures::stream::iter_ok;
use futures_ext::{BoxStream, StreamExt};
use std::collections::HashSet;

use heads::Heads;

/// Generic, in-memory heads store backed by a HashSet, intended to be used in tests.
pub struct MemHeads<T: Hash + Eq + Clone> {
    heads: Mutex<HashSet<T>>,
}

impl<T: Hash + Eq + Clone + Send> MemHeads<T> {
    #[allow(dead_code)]
    pub fn new() -> Self {
        MemHeads {
            heads: Mutex::new(HashSet::new()),
        }
    }
}

impl<T: Hash + Eq + Clone + Send + 'static> Heads for MemHeads<T> {
    type Key = T;
    type Error = !;

    type Effect = FutureResult<(), Self::Error>;
    type Bool = FutureResult<bool, Self::Error>;
    type Heads = BoxStream<Self::Key, Self::Error>;

    fn add(&self, head: &Self::Key) -> Self::Effect {
        self.heads.lock().unwrap().insert(head.clone());
        ok(())
    }

    fn remove(&self, head: &Self::Key) -> Self::Effect {
        self.heads.lock().unwrap().remove(head);
        ok(())
    }

    fn is_head(&self, head: &Self::Key) -> Self::Bool {
        ok(self.heads.lock().unwrap().contains(head))
    }

    fn heads(&self) -> Self::Heads {
        let guard = self.heads.lock().unwrap();
        let heads = (*guard).clone();
        iter_ok::<_, !>(heads).boxify()
    }
}

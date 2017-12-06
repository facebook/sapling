// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]

extern crate failure;
extern crate futures;
extern crate futures_ext;
extern crate heads;
extern crate mercurial_types;

use std::collections::HashSet;
use std::sync::Mutex;

use failure::Error;
use futures::future::ok;
use futures::stream::iter_ok;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};


use heads::Heads;
use mercurial_types::NodeHash;

/// Generic, in-memory heads store backed by a HashSet, intended to be used in tests.
pub struct MemHeads {
    heads: Mutex<HashSet<NodeHash>>,
}

impl MemHeads {
    #[allow(dead_code)]
    pub fn new() -> Self {
        MemHeads {
            heads: Mutex::new(HashSet::new()),
        }
    }
}

impl Heads for MemHeads {
    fn add(&self, head: &NodeHash) -> BoxFuture<(), Error> {
        self.heads.lock().unwrap().insert(head.clone());
        ok(()).boxify()
    }

    fn remove(&self, head: &NodeHash) -> BoxFuture<(), Error> {
        self.heads.lock().unwrap().remove(head);
        ok(()).boxify()
    }

    fn is_head(&self, head: &NodeHash) -> BoxFuture<bool, Error> {
        ok(self.heads.lock().unwrap().contains(head)).boxify()
    }

    fn heads(&self) -> BoxStream<NodeHash, Error> {
        let guard = self.heads.lock().unwrap();
        let heads = (*guard).clone();
        iter_ok(heads).boxify()
    }
}

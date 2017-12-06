// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;

extern crate mercurial_types;

use failure::Error;
use futures_ext::{BoxFuture, BoxStream};

use mercurial_types::NodeHash;

/// Trait representing the interface to a heads store, which more generally is just
/// a set of commit identifiers.
pub trait Heads: Send + 'static {
    // Heads are not guaranteed to be returned in any particular order. Heads that exist for
    // the entire duration of the traversal are guaranteed to appear at least once.

    fn add(&self, &NodeHash) -> BoxFuture<(), Error>;
    fn remove(&self, &NodeHash) -> BoxFuture<(), Error>;
    fn is_head(&self, &NodeHash) -> BoxFuture<bool, Error>;
    fn heads(&self) -> BoxStream<NodeHash, Error>;
}

impl Heads for Box<Heads> {
    fn add(&self, head: &NodeHash) -> BoxFuture<(), Error> {
        self.as_ref().add(head)
    }

    fn remove(&self, head: &NodeHash) -> BoxFuture<(), Error> {
        self.as_ref().remove(head)
    }

    fn is_head(&self, hash: &NodeHash) -> BoxFuture<bool, Error> {
        self.as_ref().is_head(hash)
    }

    fn heads(&self) -> BoxStream<NodeHash, Error> {
        self.as_ref().heads()
    }
}

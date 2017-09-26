// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate futures;

use futures::{Future, Stream};
use std::error;

/// Trait representing the interface to a heads store, which more generally is just
/// a set of commit identifiers.
pub trait Heads: Send + 'static {
    type Key: Send + 'static;
    type Error: error::Error + Send + 'static;

    // Heads are not guaranteed to be returned in any particular order. Heads that exist for
    // the entire duration of the traversal are guaranteed to appear at least once.
    type Heads: Stream<Item = Self::Key, Error = Self::Error> + Send + 'static;
    type Bool: Future<Item = bool, Error = Self::Error> + Send + 'static;
    type Effect: Future<Item = (), Error = Self::Error> + Send + 'static;

    fn add(&self, &Self::Key) -> Self::Effect;
    fn remove(&self, &Self::Key) -> Self::Effect;
    fn is_head(&self, &Self::Key) -> Self::Bool;
    fn heads(&self) -> Self::Heads;
}

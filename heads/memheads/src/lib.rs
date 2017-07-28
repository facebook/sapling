// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate heads;
#[macro_use]
extern crate error_chain;
extern crate futures;

use std::hash::Hash;
use std::sync::Mutex;

use futures::future::{FutureResult, ok};
use futures::stream::{BoxStream, Stream, iter};
use std::collections::HashSet;

use heads::Heads;

mod errors {
    // Create Error, ErrorKind, ResultExt, and Result types.
    error_chain!{}
}
use errors::*;

/// Generic, in-memory heads store backed by a HashSet, intended to be used in tests.
pub struct MemHeads<T: Hash + Eq + Clone> {
    heads: Mutex<HashSet<T>>,
}

impl<T: Hash + Eq + Clone + Send> MemHeads<T> {
    fn new() -> Self {
        MemHeads { heads: Mutex::new(HashSet::new()) }
    }
}

impl<T: Hash + Eq + Clone + Send + 'static> Heads for MemHeads<T> {
    type Key = T;
    type Error = Error;

    type Unit = FutureResult<(), Self::Error>;
    type Bool = FutureResult<bool, Self::Error>;
    type Heads = BoxStream<Self::Key, Self::Error>;

    fn add(&self, head: &Self::Key) -> Self::Unit {
        self.heads.lock().unwrap().insert(head.clone());
        ok(())
    }

    fn remove(&self, head: &Self::Key) -> Self::Unit {
        self.heads.lock().unwrap().remove(head);
        ok(())
    }

    fn is_head(&self, head: &Self::Key) -> Self::Bool {
        ok(self.heads.lock().unwrap().contains(head))
    }

    fn heads(&self) -> Self::Heads {
        let guard = self.heads.lock().unwrap();
        let heads = (*guard).clone();
        iter(heads.into_iter().map(|head| Ok(head))).boxed()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::Future;
    use futures::Stream;

    #[test]
    fn test_heads() {
        let heads = MemHeads::new();
        let empty: Vec<&str> = Vec::new();
        assert_eq!(heads.heads().collect().wait().unwrap(), empty);

        assert!(!heads.is_head(&"foo").wait().unwrap());
        assert!(!heads.is_head(&"bar").wait().unwrap());
        assert!(!heads.is_head(&"baz").wait().unwrap());

        heads.add(&"foo").wait().unwrap();
        heads.add(&"bar").wait().unwrap();

        assert!(heads.is_head(&"foo").wait().unwrap());
        assert!(heads.is_head(&"bar").wait().unwrap());
        assert!(!heads.is_head(&"baz").wait().unwrap());

        let mut result = heads.heads().collect().wait().unwrap();
        result.sort();

        assert_eq!(result, vec!["bar", "foo"]);

        heads.remove(&"foo").wait().unwrap();
        heads.remove(&"bar").wait().unwrap();
        heads.remove(&"baz").wait().unwrap(); // Removing non-existent head should not panic.

        assert_eq!(heads.heads().collect().wait().unwrap(), empty);
    }
}

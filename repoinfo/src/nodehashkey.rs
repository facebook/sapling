// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use asyncmemo::Weight;
use heapsize::HeapSizeOf;
use mercurial_types::HgNodeHash;

use ptrwrap::PtrWrap;

#[derive(Debug)]
pub struct Key<R>(pub PtrWrap<R>, pub HgNodeHash);

impl<R> Clone for Key<R> {
    fn clone(&self) -> Self {
        Key(self.0.clone(), self.1)
    }
}

impl<R> Eq for Key<R> {}
impl<R> PartialEq for Key<R> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0) && self.1.eq(&other.1)
    }
}

impl<R> Hash for Key<R> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
        self.1.hash(state);
    }
}

impl<R> HeapSizeOf for Key<R> {
    fn heap_size_of_children(&self) -> usize {
        self.0.heap_size_of_children() + self.1.heap_size_of_children()
    }
}

impl<R> Weight for Key<R> {
    fn get_weight(&self) -> usize {
        self.heap_size_of_children()
    }
}

impl<'a, R> From<(&'a Arc<R>, HgNodeHash)> for Key<R> {
    fn from((repo, hash): (&'a Arc<R>, HgNodeHash)) -> Self {
        Key(From::from(repo), hash)
    }
}

impl<'a, R> From<(&'a PtrWrap<R>, HgNodeHash)> for Key<R> {
    fn from((repo, hash): (&'a PtrWrap<R>, HgNodeHash)) -> Self {
        Key(repo.clone(), hash)
    }
}

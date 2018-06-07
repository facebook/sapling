// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Construct generation numbers for changesets within a repo
//!
//! A generation number for a changeset is 1 + max(parents, 0). This number is computed for each
//! changeset and memoized for efficiency.

use std::{usize, u64};
use std::mem;
use std::sync::Arc;

use failure::{err_msg, Error};
use futures::IntoFuture;
use futures::future::{Either, Future};

use futures_ext::FutureExt;

use asyncmemo::{Asyncmemo, Filler, Weight};
use blobrepo::BlobRepo;
use mercurial_types::{HgChangesetId, HgNodeHash, NULL_HASH};

use nodehashkey::Key;

/// Generation number
///
/// The generation number for a changeset is defined as the max of the changeset's parents'
/// generation number plus 1; if there are no parents then it's 1.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, HeapSizeOf)]
pub struct Generation(u64);

impl Weight for Generation {
    fn get_weight(&self) -> usize {
        mem::size_of::<Self>()
    }
}

impl Generation {
    /// Creates new generation number
    pub fn new(gen: u64) -> Self {
        Generation(gen)
    }

    /// Create a maximum possible generation number
    pub fn max_gen() -> Self {
        Generation(u64::MAX)
    }
}

/// Cache of generation numbers
///
/// Allows generation numbers for a changeset to be computed lazily and cached.
pub struct RepoGenCache {
    cache: Asyncmemo<GenFiller>,
}

impl Clone for RepoGenCache {
    fn clone(&self) -> Self {
        Self {
            cache: self.cache.clone(),
        }
    }
}

impl RepoGenCache {
    /// Construct a new `RepoGenCache`, bounded to `sizelimit` bytes.
    pub fn new(sizelimit: usize) -> Self {
        RepoGenCache {
            cache: Asyncmemo::with_limits(GenFiller::new(), usize::MAX, sizelimit),
        }
    }

    /// Get a `Future` for a `Generation` number for a given changeset in a repo.
    pub fn get(
        &self,
        repo: &Arc<BlobRepo>,
        nodeid: HgNodeHash,
    ) -> impl Future<Item = Generation, Error = Error> + Send {
        if nodeid == NULL_HASH {
            Either::A(Ok(Generation(0)).into_future())
        } else {
            Either::B(self.cache.get((repo, nodeid.clone())))
        }
    }
}

pub struct GenFiller {}

impl GenFiller {
    fn new() -> Self {
        GenFiller {}
    }
}

impl Filler for GenFiller {
    type Key = Key<BlobRepo>;
    type Value = Box<Future<Item = Generation, Error = Error> + Send>;

    fn fill(
        &self,
        _cache: &Asyncmemo<Self>,
        &Key(ref repo, ref nodeid): &Self::Key,
    ) -> Self::Value {
        let cs = HgChangesetId::new(*nodeid);
        repo.get_generation_number(&cs)
            .and_then(move |genopt| genopt.ok_or_else(|| err_msg(format!("{} not found", cs))))
            .map(|gen| Generation(gen))
            .boxify()
    }
}

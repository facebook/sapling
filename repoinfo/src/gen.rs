// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Construct generation numbers for changesets within a repo
//!
//! A generation number for a changeset is 1 + max(parents, 0). This number is computed for each
//! changeset and memoized for efficiency.

use std::cmp;
use std::sync::Arc;
use std::usize;

use failure::Error;
use futures::future::{self, Future};
use futures::stream::{self, Stream};

use asyncmemo::{Asyncmemo, Filler};
use blobrepo::BlobRepo;
use mercurial_types::NodeHash;

use nodehashkey::Key;

/// Generation number
///
/// The generation number for a changeset is defined as the max of the changeset's parents'
/// generation number plus 1; if there are no parents then it's 1.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, HeapSizeOf)]
pub struct Generation(u64);

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
        nodeid: NodeHash,
    ) -> impl Future<Item = Generation, Error = Error> + Send {
        self.cache.get((repo, nodeid))
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

    fn fill(&self, cache: &Asyncmemo<Self>, &Key(ref repo, ref nodeid): &Self::Key) -> Self::Value {
        let parents = repo
            .get_changeset_by_nodeid(nodeid) // Future<Changeset>
            .map(|cs| stream::iter_ok(cs.parents().into_iter()))
            .flatten_stream(); // Stream<NodeHash>

        let gen = parents
            .map({
                let repo = repo.clone();
                let cache = cache.clone();

                // recursive call to get gen for parent(s)
                move |p| cache.get((&repo, p))
            }) // Stream<Future<Generation>>
            .buffer_unordered(2) // (up to 2 parents) Stream<Generation>
            .fold(Generation(0), |g, s| future::ok::<_, Error>(cmp::max(g, s)))
            .map(|Generation(g)| Generation(g + 1)); // Future<Generation>

        Box::new(gen) as Box<Future<Item = Generation, Error = Error> + Send + 'static>
    }
}

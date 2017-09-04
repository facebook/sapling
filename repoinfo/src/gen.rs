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
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::Arc;
use std::usize;

use futures::future::{self, Future};
use futures::stream::{self, Stream};
use heapsize::HeapSizeOf;

use asyncmemo::{Asyncmemo, Filler, MemoFuture};
use mercurial_types::{NodeHash, Repo};

use ptrwrap::PtrWrap;

/// Generation number
///
/// The generation number for a changeset is defined as the max of the changeset's parents'
/// generation number plus 1; if there are no parents then it's 1.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, HeapSizeOf)]
pub struct Generation(usize);

/// Cache of generation numbers
///
/// Allows generation numbers for a changeset to be computed lazily and cached.
pub struct RepoGenCache<R>
where
    R: Repo,
{
    cache: Asyncmemo<GenFiller<R>>,
}

impl<R> Clone for RepoGenCache<R>
where
    R: Repo,
{
    fn clone(&self) -> Self {
        Self {
            cache: self.cache.clone(),
        }
    }
}

impl<R> RepoGenCache<R>
where
    R: Repo,
{
    /// Construct a new `RepoGenCache`, bounded to `sizelimit` bytes.
    pub fn new(sizelimit: usize) -> Self {
        RepoGenCache {
            cache: Asyncmemo::with_limits(GenFiller::new(), usize::MAX, sizelimit),
        }
    }

    /// Get a `Future` for a `Generation` number for a given changeset in a repo.
    pub fn get<AR>(&self, repo: AR, nodeid: NodeHash) -> MemoFuture<GenFiller<R>>
    where
        AR: AsRef<Arc<R>>,
    {
        self.cache.get((repo.as_ref(), nodeid))
    }
}

pub struct GenFiller<R> {
    _phantom: PhantomData<R>,
}

impl<R> GenFiller<R> {
    fn new() -> Self {
        GenFiller {
            _phantom: PhantomData,
        }
    }
}

#[derive(Debug)]
pub struct Key<R>(PtrWrap<R>, NodeHash);

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

impl<'a, R> From<(&'a Arc<R>, NodeHash)> for Key<R> {
    fn from((repo, hash): (&'a Arc<R>, NodeHash)) -> Self {
        Key(From::from(repo), hash)
    }
}

impl<'a, R> From<(&'a PtrWrap<R>, NodeHash)> for Key<R> {
    fn from((repo, hash): (&'a PtrWrap<R>, NodeHash)) -> Self {
        Key(repo.clone(), hash)
    }
}

impl<R> Filler for GenFiller<R>
where
    R: Repo,
{
    type Key = Key<R>;
    type Value = Box<Future<Item = Generation, Error = R::Error>>;

    fn fill(&self, cache: &Asyncmemo<Self>, &Key(ref repo, ref nodeid): &Self::Key) -> Self::Value {
        let parents = repo
            .get_changeset_by_nodeid(nodeid) // Future<Changeset>
            .map(|cs| stream::iter(cs.parents().into_iter().map(Result::Ok)))
            .flatten_stream(); // Stream<NodeHash>

        let gen = parents
            .map({
                let repo = repo.clone();
                let cache = cache.clone();

                // recursive call to get gen for parent(s)
                move |p| cache.get((&repo, p))
            }) // Stream<Future<Generation>>
            .buffer_unordered(2) // (up to 2 parents) Stream<Generation>
            .fold(Generation(0), |g, s| future::ok(cmp::max(g, s)))
            .map(|Generation(g)| Generation(g + 1)); // Future<Generation>

        Box::new(gen) as Box<Future<Item = Generation, Error = R::Error> + 'static>
    }
}

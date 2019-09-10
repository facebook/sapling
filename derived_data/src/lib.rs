// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use blobrepo::BlobRepo;
use context::CoreContext;
use failure::Error;
use futures_ext::{BoxFuture, FutureExt};
use lock_ext::LockExt;
use mononoke_types::{BonsaiChangeset, ChangesetId};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

mod derive_impl;

/// Trait for the data that can be derived from bonsai changeset.
/// Examples of that are hg changeset id, unodes root manifest id, git changeset ids etc
pub trait BonsaiDerived: Sized + 'static + Send + Sync + Clone {
    /// Name of derived data
    ///
    /// Should be unique string (among derived data types), which is used to identify or
    /// name data (for example lease keys) assoicated with particular derived data type.
    const NAME: &'static str;

    /// Defines how to derive new representation for bonsai having derivations
    /// for parents and having a current bonsai object.
    ///
    /// Note that if any data has to be persistently stored in blobstore, mysql or any other store
    /// then it's responsiblity of implementor of `derive_from_parents()` to save it.
    /// For example, to derive HgChangesetId we also need to derive all filenodes and all manifests
    /// and then store them in blobstore. Derived data library is only responsible for
    /// updating BonsaiDerivedMapping.
    fn derive_from_parents(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> BoxFuture<Self, Error>;

    /// This function is the entrypoint for changeset derivation, it converts
    /// bonsai representation to derived one by calling derive_from_parents(), and saves mapping
    /// from csid -> BonsaiDerived in BonsaiDerivedMapping
    fn derive<Mapping>(
        ctx: CoreContext,
        repo: BlobRepo,
        mapping: Mapping,
        csid: ChangesetId,
    ) -> BoxFuture<Self, Error>
    where
        Mapping: BonsaiDerivedMapping<Value = Self> + Send + Sync + Clone + 'static,
    {
        derive_impl::derive_impl::<Self, Mapping>(ctx, repo, mapping, csid).boxify()
    }
}

/// After derived data was generated then it will be stored in BonsaiDerivedMapping, which is
/// normally a persistent store. This is used to avoid regenerating the same derived data over
/// and over again.
pub trait BonsaiDerivedMapping: Send + Sync {
    type Value: BonsaiDerived;

    /// Fetches mapping from bonsai changeset ids to generated value
    fn get(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<HashMap<ChangesetId, Self::Value>, Error>;

    /// Saves mapping between bonsai changeset and derived data id
    fn put(&self, ctx: CoreContext, csid: ChangesetId, id: Self::Value) -> BoxFuture<(), Error>;
}

impl<Mapping: BonsaiDerivedMapping> BonsaiDerivedMapping for Arc<Mapping> {
    type Value = Mapping::Value;

    fn get(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<HashMap<ChangesetId, Self::Value>, Error> {
        (**self).get(ctx, csids)
    }

    fn put(&self, ctx: CoreContext, csid: ChangesetId, id: Self::Value) -> BoxFuture<(), Error> {
        (**self).put(ctx, csid, id)
    }
}

/// This mapping can be used when we want to ignore values before it was put
/// again for some specific set of commits. It is useful when we want either
/// re-backfill derived data or investigate performance problems.
#[derive(Clone)]
pub struct RegenerateMapping<M> {
    regenerate: Arc<Mutex<HashSet<ChangesetId>>>,
    base: M,
}

impl<M> RegenerateMapping<M> {
    pub fn new(base: M) -> Self {
        Self {
            regenerate: Default::default(),
            base,
        }
    }

    pub fn regenerate<I: IntoIterator<Item = ChangesetId>>(&self, csids: I) {
        self.regenerate.with(|regenerate| regenerate.extend(csids))
    }
}

impl<M> BonsaiDerivedMapping for RegenerateMapping<M>
where
    M: BonsaiDerivedMapping,
{
    type Value = M::Value;

    fn get(
        &self,
        ctx: CoreContext,
        mut csids: Vec<ChangesetId>,
    ) -> BoxFuture<HashMap<ChangesetId, Self::Value>, Error> {
        self.regenerate
            .with(|regenerate| csids.retain(|id| !regenerate.contains(&id)));
        self.base.get(ctx, csids)
    }

    fn put(&self, ctx: CoreContext, csid: ChangesetId, id: Self::Value) -> BoxFuture<(), Error> {
        self.regenerate.with(|regenerate| regenerate.remove(&csid));
        self.base.put(ctx, csid, id)
    }
}

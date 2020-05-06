/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Error;
use async_trait::async_trait;
use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use futures::{
    compat::Future01CompatExt, stream, FutureExt, StreamExt, TryFutureExt, TryStreamExt,
};
use futures_ext::{BoxFuture, FutureExt as OldFutureExt};
use lock_ext::LockExt;
use mononoke_types::{BonsaiChangeset, ChangesetId, RepositoryId};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use thiserror::Error;

pub mod derive_impl;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Mode {
    /// This mode should almost always be preferred
    OnlyIfEnabled,
    /// This mode should rarely be used, perhaps only for backfilling type of derived data
    /// which is not enabled in this repo yet
    Unsafe,
}

#[derive(Debug, Error)]
pub enum DeriveError {
    #[error("Derivation of {0} is not enabled for repo {1}")]
    Disabled(&'static str, RepositoryId),
    #[error("{0}")]
    Error(#[from] Error),
}

/// Trait for the data that can be derived from bonsai changeset.
/// Examples of that are hg changeset id, unodes root manifest id, git changeset ids etc
#[async_trait]
pub trait BonsaiDerived: Sized + 'static + Send + Sync + Clone {
    /// Name of derived data
    ///
    /// Should be unique string (among derived data types), which is used to identify or
    /// name data (for example lease keys) assoicated with particular derived data type.
    const NAME: &'static str;

    type Mapping: BonsaiDerivedMapping<Value = Self>;

    /// Get mapping associated with this derived data type.
    fn mapping(ctx: &CoreContext, repo: &BlobRepo) -> Self::Mapping;

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
    ///
    /// This function fails immediately if this type of derived data is not enabled for this repo.
    fn derive(ctx: CoreContext, repo: BlobRepo, csid: ChangesetId) -> BoxFuture<Self, DeriveError> {
        let mapping = Self::mapping(&ctx, &repo);
        derive_impl::derive_impl::<Self, Self::Mapping>(
            ctx,
            repo,
            mapping,
            csid,
            Mode::OnlyIfEnabled,
        )
        .boxify()
    }

    /// Derives derived data even if it's disabled in the config. Should normally
    /// be used only for backfilling.
    fn derive_with_mode(
        ctx: CoreContext,
        repo: BlobRepo,
        csid: ChangesetId,
        mode: Mode,
    ) -> BoxFuture<Self, DeriveError> {
        let mapping = Self::mapping(&ctx, &repo);
        derive_impl::derive_impl::<Self, Self::Mapping>(ctx, repo, mapping, csid, mode).boxify()
    }

    /// Returns min(number of ancestors of `csid` to be derived, `limit`)
    ///
    /// This function fails immediately if derived data is not enabled for this repo.
    async fn count_underived(
        ctx: &CoreContext,
        repo: &BlobRepo,
        csid: &ChangesetId,
        limit: u64,
    ) -> Result<u64, DeriveError> {
        let mapping = Self::mapping(&ctx, &repo);
        let underived = derive_impl::find_underived::<Self, Self::Mapping>(
            ctx,
            repo,
            &mapping,
            csid,
            Some(limit),
            Mode::OnlyIfEnabled,
        )
        .await?;
        Ok(underived.len() as u64)
    }

    fn is_derived(
        ctx: &CoreContext,
        repo: &BlobRepo,
        csid: &ChangesetId,
    ) -> BoxFuture<bool, DeriveError> {
        // TODO(stash): asyncify to avoid clone()
        cloned!(ctx, repo, csid);
        async move {
            let count = Self::count_underived(&ctx, &repo, &csid, 1).await?;
            Ok(count == 0)
        }
        .boxed()
        .compat()
        .boxify()
    }

    /// This method might be overridden by BonsaiDerived implementors if there's a more efficienta
    /// way to derive a batch of commits
    async fn batch_derive<'a, Iter>(
        ctx: &CoreContext,
        repo: &BlobRepo,
        csids: Iter,
    ) -> Result<HashMap<ChangesetId, Self>, Error>
    where
        Iter: IntoIterator<Item = ChangesetId> + Send,
        Iter::IntoIter: Send,
    {
        let iter = csids.into_iter();
        stream::iter(iter.map(|cs_id| async move {
            let derived = Self::derive(ctx.clone(), repo.clone(), cs_id)
                .compat()
                .await?;
            Ok((cs_id, derived))
        }))
        .buffered(100)
        .try_collect::<HashMap<_, _>>()
        .await
    }
}

/// After derived data was generated then it will be stored in BonsaiDerivedMapping, which is
/// normally a persistent store. This is used to avoid regenerating the same derived data over
/// and over again.
pub trait BonsaiDerivedMapping: Send + Sync + Clone {
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

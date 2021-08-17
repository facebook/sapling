/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use auto_impl::auto_impl;
use context::CoreContext;
use fbinit::FacebookInit;
use futures_stats::TimedFutureExt;
use lock_ext::LockExt;
use mononoke_types::ChangesetId;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::BonsaiDerivable;

#[derive(Clone)]
pub struct BonsaiDerivedMappingContainer<Derivable>
where
    Derivable: BonsaiDerivable,
{
    mapping: Arc<dyn BonsaiDerivedMapping<Value = Derivable>>,
    scuba: MononokeScubaSampleBuilder,
}

impl<Derivable> BonsaiDerivedMappingContainer<Derivable>
where
    Derivable: BonsaiDerivable,
{
    pub fn new(
        fb: FacebookInit,
        repo_name: &str,
        scuba_table: Option<&str>,
        mapping: Arc<dyn BonsaiDerivedMapping<Value = Derivable>>,
    ) -> Self {
        let scuba = match scuba_table {
            Some(scuba_table) => {
                let mut builder = MononokeScubaSampleBuilder::new(fb, scuba_table);
                builder.add_common_server_data();
                builder.add("derived_data", Derivable::NAME);
                builder.add("reponame", repo_name);
                builder
            }
            None => MononokeScubaSampleBuilder::with_discard(),
        };
        Self { mapping, scuba }
    }

    /// Fetch the mapped values for a set of changesets.
    pub async fn get(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Derivable>> {
        self.mapping.get(ctx, csids).await
    }

    /// Store a new mapping between a bonsai changeset and a derived value.
    /// The new value is also logged to scuba.
    pub async fn put(&self, ctx: &CoreContext, csid: ChangesetId, value: &Derivable) -> Result<()> {
        let (stats, res) = self.mapping.put(ctx, csid, value).timed().await;
        let mut scuba = self.scuba.clone();
        crate::logging::log_mapping_insertion::<Derivable>(ctx, &mut scuba, &stats, &res, &value);
        res
    }

    pub fn options(&self) -> <Derivable as BonsaiDerivable>::Options {
        self.mapping.options()
    }
}

#[async_trait]
#[auto_impl(Arc)]
pub trait BonsaiDerivedMapping: Send + Sync {
    type Value: BonsaiDerivable;

    /// Fetch the mapped values for a set of changesets.
    async fn get(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::Value>>;

    /// Store a new mapping between a bonsai changeset and a derived value.
    async fn put(&self, ctx: &CoreContext, csid: ChangesetId, value: &Self::Value) -> Result<()>;

    /// Get the derivation options that apply for this mapping.
    fn options(&self) -> <Self::Value as BonsaiDerivable>::Options;
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

#[async_trait]
impl<M> BonsaiDerivedMapping for RegenerateMapping<M>
where
    M: BonsaiDerivedMapping,
{
    type Value = M::Value;

    async fn get(
        &self,
        ctx: &CoreContext,
        mut csids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::Value>> {
        self.regenerate
            .with(|regenerate| csids.retain(|id| !regenerate.contains(&id)));
        self.base.get(ctx, csids).await
    }

    async fn put(&self, ctx: &CoreContext, csid: ChangesetId, id: &Self::Value) -> Result<()> {
        self.regenerate.with(|regenerate| regenerate.remove(&csid));
        self.base.put(ctx, csid, id).await
    }

    fn options(&self) -> <M::Value as BonsaiDerivable>::Options {
        self.base.options()
    }
}

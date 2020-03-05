/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Implementations for wrappers that enable dynamic dispatch. Add more as necessary.

use std::sync::Arc;

use anyhow::Error;
use context::CoreContext;
use futures_ext::BoxFuture;
use mononoke_types::{
    ChangesetId, ChangesetIdPrefix, ChangesetIdsResolvedFromPrefix, RepositoryId,
};

use crate::{ChangesetEntry, ChangesetInsert, Changesets};

impl Changesets for Arc<dyn Changesets> {
    fn add(&self, ctx: CoreContext, cs: ChangesetInsert) -> BoxFuture<bool, Error> {
        (**self).add(ctx, cs)
    }

    fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<ChangesetEntry>, Error> {
        (**self).get(ctx, repo_id, cs_id)
    }

    fn get_many(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_ids: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetEntry>, Error> {
        (**self).get_many(ctx, repo_id, cs_ids)
    }

    fn get_many_by_prefix(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_prefix: ChangesetIdPrefix,
        limit: usize,
    ) -> BoxFuture<ChangesetIdsResolvedFromPrefix, Error> {
        (**self).get_many_by_prefix(ctx, repo_id, cs_prefix, limit)
    }

    fn prime_cache(&self, ctx: &CoreContext, changesets: &[ChangesetEntry]) {
        (**self).prime_cache(ctx, changesets)
    }
}

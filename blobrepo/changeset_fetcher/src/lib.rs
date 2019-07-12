// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use changesets::Changesets;
use context::CoreContext;
use failure::{err_msg, Error};
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::{ChangesetId, Generation, RepositoryId};

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

/// Trait that knows how to fetch DAG info about commits. Primary user is revsets
/// Concrete implementation may add more efficient caching logic to make request faster
pub trait ChangesetFetcher: Send + Sync {
    fn get_generation_number(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> BoxFuture<Generation, Error>;

    fn get_parents(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> BoxFuture<Vec<ChangesetId>, Error>;

    fn get_stats(&self) -> HashMap<String, Box<dyn Any>> {
        HashMap::new()
    }
}

/// Simplest ChangesetFetcher implementation which is just a wrapper around `Changesets` object
pub struct SimpleChangesetFetcher {
    changesets: Arc<dyn Changesets>,
    repo_id: RepositoryId,
}

impl SimpleChangesetFetcher {
    pub fn new(changesets: Arc<dyn Changesets>, repo_id: RepositoryId) -> Self {
        Self {
            changesets,
            repo_id,
        }
    }
}

impl ChangesetFetcher for SimpleChangesetFetcher {
    fn get_generation_number(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> BoxFuture<Generation, Error> {
        self.changesets
            .get(ctx, self.repo_id.clone(), cs_id.clone())
            .and_then(move |maybe_cs| {
                maybe_cs.ok_or_else(|| err_msg(format!("{} not found", cs_id)))
            })
            .map(|cs| Generation::new(cs.gen))
            .boxify()
    }

    fn get_parents(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> BoxFuture<Vec<ChangesetId>, Error> {
        self.changesets
            .get(ctx, self.repo_id.clone(), cs_id.clone())
            .and_then(move |maybe_cs| {
                maybe_cs.ok_or_else(|| err_msg(format!("{} not found", cs_id)))
            })
            .map(|cs| cs.parents)
            .boxify()
    }
}

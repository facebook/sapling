/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

///! Segmented Changelog
///!
///! This represents an implementation for the core commit graph that we have
///! in a given repository. It provides algorithms over the commit graph.
use std::sync::Arc;

use anyhow::{format_err, Result};
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::ChangesetId;

mod builder;
mod bundle;
mod dag;
mod iddag;
mod idmap;
mod on_demand;
mod seeder;
mod sql_types;
mod types;

#[cfg(test)]
mod tests;

pub use crate::builder::SegmentedChangelogBuilder;

// TODO(T74420661): use `thiserror` to represent error case

#[async_trait]
pub trait SegmentedChangelog: Send + Sync {
    /// Get the identifier of a commit given it's commit graph location.
    ///
    /// The client using segmented changelog will have only a set of identifiers for the commits in
    /// the graph. To retrieve the identifier of an commit that is now known they will provide a
    /// known descendant and the distance from the known commit to the commit we inquire about.
    async fn location_to_changeset_id(
        &self,
        ctx: &CoreContext,
        known: ChangesetId,
        distance: u64,
    ) -> Result<ChangesetId> {
        self.location_to_many_changeset_ids(ctx, known, distance, 1)
            .await
            .map(|v| v[0])
    }

    /// Get identifiers of a continuous set of commit given their commit graph location.
    ///
    /// Similar to `location_to_changeset_id` but instead of returning the ancestor that is
    /// `distance` away from the `known` commit, it returns `count` ancestors following the parents.
    /// It is expected that all but the last ancestor will have a single parent.
    async fn location_to_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        known: ChangesetId,
        distance: u64,
        count: u64,
    ) -> Result<Vec<ChangesetId>>;
}

#[async_trait]
impl SegmentedChangelog for Arc<dyn SegmentedChangelog> {
    async fn location_to_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        known: ChangesetId,
        distance: u64,
        count: u64,
    ) -> Result<Vec<ChangesetId>> {
        (**self)
            .location_to_many_changeset_ids(ctx, known, distance, count)
            .await
    }
}

pub struct DisabledSegmentedChangelog;

impl DisabledSegmentedChangelog {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl SegmentedChangelog for DisabledSegmentedChangelog {
    async fn location_to_many_changeset_ids(
        &self,
        _ctx: &CoreContext,
        _known: ChangesetId,
        _distance: u64,
        _count: u64,
    ) -> Result<Vec<ChangesetId>> {
        // TODO(T74420661): use `thiserror` to represent error case
        Err(format_err!(
            "Segmented Changelog is not enabled for this repo",
        ))
    }
}

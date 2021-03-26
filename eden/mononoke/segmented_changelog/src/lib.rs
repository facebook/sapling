/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

//! Segmented Changelog
//!
//! This represents an implementation for the core commit graph that we have
//! in a given repository. It provides algorithms over the commit graph.
use std::collections::HashMap;

use anyhow::{format_err, Result};
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::ChangesetId;

mod builder;
mod iddag;
mod idmap;
mod logging;
mod manager;
mod on_demand;
mod owned;
mod periodic_reload;
mod read_only;
mod seeder;
mod sql_types;
mod tailer;
pub mod types;
mod update;
mod version_store;

#[cfg(test)]
mod tests;

pub use segmented_changelog_types::{
    dag, ArcSegmentedChangelog, CloneData, FirstAncestorConstraint, FlatSegment, Group,
    InProcessIdDag, Location, PreparedFlatSegments, SegmentedChangelog, StreamCloneData, Vertex,
};

pub use crate::builder::{new_server_segmented_changelog, SegmentedChangelogSqlConnections};
pub use crate::seeder::SegmentedChangelogSeeder;
pub use crate::tailer::SegmentedChangelogTailer;

// public for benchmarking
pub use crate::idmap::{ConcurrentMemIdMap, IdMap};

// TODO(T74420661): use `thiserror` to represent error case

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
        _location: Location<ChangesetId>,
        _count: u64,
    ) -> Result<Vec<ChangesetId>> {
        // TODO(T74420661): use `thiserror` to represent error case
        Err(format_err!(
            "Segmented Changelog is not enabled for this repo",
        ))
    }

    async fn clone_data(&self, _ctx: &CoreContext) -> Result<CloneData<ChangesetId>> {
        Err(format_err!(
            "Segmented Changelog is not enabled for this repo",
        ))
    }

    async fn full_idmap_clone_data(
        &self,
        _ctx: &CoreContext,
    ) -> Result<StreamCloneData<ChangesetId>> {
        Err(format_err!(
            "Segmented Changelog is not enabled for this repo",
        ))
    }

    async fn many_changeset_ids_to_locations(
        &self,
        _ctx: &CoreContext,
        _client_head: ChangesetId,
        _cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Location<ChangesetId>>> {
        Err(format_err!(
            "Segmented Changelog is not enabled for this repo",
        ))
    }
}

#[macro_export]
macro_rules! segmented_changelog_delegate {
    ($type:ident, |&$self:ident, $ctx:ident: &CoreContext,| $delegate:block) => {
        // the difference in the pattern is the extra comma after Context
        segmented_changelog_delegate!($type, |&$self, $ctx: &CoreContext| $delegate);
    };
    ($type:ident, |&$self:ident, $ctx:ident: &CoreContext| $delegate:block) => {
        #[async_trait]
        impl SegmentedChangelog for $type {
            async fn location_to_many_changeset_ids(
                &$self,
                $ctx: &CoreContext,
                location: Location<ChangesetId>,
                count: u64,
            ) -> Result<Vec<ChangesetId>> {
                let delegate = $delegate;
                delegate
                    .location_to_many_changeset_ids($ctx, location, count)
                    .await
            }

            async fn clone_data(&$self, $ctx: &CoreContext) -> Result<CloneData<ChangesetId>> {
                let delegate = $delegate;
                delegate.clone_data($ctx).await
            }

            async fn full_idmap_clone_data(
                &$self,
                $ctx: &CoreContext,
            ) -> Result<StreamCloneData<ChangesetId>> {
                let delegate = $delegate;
                delegate.full_idmap_clone_data($ctx).await
            }

            async fn many_changeset_ids_to_locations(
                &$self,
                $ctx: &CoreContext,
                client_head: ChangesetId,
                cs_ids: Vec<ChangesetId>,
            ) -> Result<HashMap<ChangesetId, Location<ChangesetId>>> {
                let delegate = $delegate;
                delegate
                    .many_changeset_ids_to_locations($ctx, client_head, cs_ids)
                    .await
            }
        }
    };
}

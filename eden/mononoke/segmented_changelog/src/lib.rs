/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Segmented Changelog
//!
//! This represents an implementation for the core commit graph that we have
//! in a given repository. It provides algorithms over the commit graph.
use std::collections::HashMap;

use anyhow::format_err;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::ChangesetId;

mod builder;
mod clone_hints;
mod copy;
mod iddag;
mod idmap;
mod logging;
mod manager;
mod on_demand;
mod owned;
mod parents;
mod periodic_reload;
mod read_only;
mod sql_types;
mod tailer;
pub mod types;
mod update;
mod version_store;

#[cfg(test)]
mod tests;

use mercurial_types::HgChangesetId;
pub use segmented_changelog_types::dag;
pub use segmented_changelog_types::ArcSegmentedChangelog;
pub use segmented_changelog_types::CloneData;
pub use segmented_changelog_types::DagId;
pub use segmented_changelog_types::DagIdSet;
pub use segmented_changelog_types::FirstAncestorConstraint;
pub use segmented_changelog_types::FlatSegment;
pub use segmented_changelog_types::Group;
pub use segmented_changelog_types::InProcessIdDag;
pub use segmented_changelog_types::Location;
pub use segmented_changelog_types::MismatchedHeadsError;
pub use segmented_changelog_types::PreparedFlatSegments;
pub use segmented_changelog_types::SegmentedChangelog;
pub use segmented_changelog_types::SegmentedChangelogArc;
pub use segmented_changelog_types::SegmentedChangelogRef;

pub use crate::builder::new_server_segmented_changelog;
pub use crate::builder::new_server_segmented_changelog_manager;
pub use crate::builder::new_test_segmented_changelog;
pub use crate::builder::SegmentedChangelogSqlConnections;
pub use crate::clone_hints::CloneHints;
pub use crate::copy::copy_segmented_changelog;
pub use crate::manager::ArcSegmentedChangelogManager;
pub use crate::manager::SegmentedChangelogManager;
pub use crate::tailer::SegmentedChangelogTailer;
pub use crate::update::seedheads_from_config;
pub use crate::update::JobType;
pub use crate::update::SeedHead;

// public for benchmarking
pub use crate::idmap::ConcurrentMemIdMap;
pub use crate::idmap::IdMap;

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

    async fn clone_data(
        &self,
        _ctx: &CoreContext,
    ) -> Result<(CloneData<ChangesetId>, HashMap<ChangesetId, HgChangesetId>)> {
        Err(format_err!(
            "Segmented Changelog is not enabled for this repo",
        ))
    }

    async fn pull_data(
        &self,
        _ctx: &CoreContext,
        _common: Vec<ChangesetId>,
        _missing: Vec<ChangesetId>,
    ) -> Result<CloneData<ChangesetId>> {
        Err(format_err!(
            "Segmented Changelog is not enabled for this repo",
        ))
    }

    async fn many_changeset_ids_to_locations(
        &self,
        _ctx: &CoreContext,
        _master_heads: Vec<ChangesetId>,
        _cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Result<Location<ChangesetId>>>> {
        Err(format_err!(
            "Segmented Changelog is not enabled for this repo",
        ))
    }

    async fn disabled(&self, _ctx: &CoreContext) -> Result<bool> {
        Ok(true)
    }

    async fn is_ancestor(
        &self,
        _ctx: &CoreContext,
        _ancestor: ChangesetId,
        _descendant: ChangesetId,
    ) -> Result<Option<bool>> {
        // None means inconclusive result, it can be returned safely
        Ok(None)
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

            async fn clone_data(
                &$self,
                $ctx: &CoreContext
            ) -> Result<(CloneData<ChangesetId>, HashMap<ChangesetId, ::mercurial_types::HgChangesetId>)> {
                let delegate = $delegate;
                delegate.clone_data($ctx).await
            }

            async fn pull_data(
                &$self,
                $ctx: &CoreContext,
                common: Vec<ChangesetId>,
                missing: Vec<ChangesetId>,
            ) -> Result<CloneData<ChangesetId>> {
                let delegate = $delegate;
                delegate.pull_data($ctx, common, missing).await
            }

            async fn many_changeset_ids_to_locations(
                &$self,
                $ctx: &CoreContext,
                master_heads: Vec<ChangesetId>,
                cs_ids: Vec<ChangesetId>,
            ) -> Result<HashMap<ChangesetId, Result<Location<ChangesetId>>>> {
                let delegate = $delegate;
                delegate
                    .many_changeset_ids_to_locations($ctx, master_heads, cs_ids)
                    .await
            }

            async fn disabled(&$self, $ctx: &CoreContext) -> Result<bool> {
                let delegate = $delegate;
                delegate.disabled($ctx).await
            }

            async fn is_ancestor(
                &$self,
                $ctx: &CoreContext,
                ancestor: ChangesetId,
                descendant: ChangesetId,
            ) -> Result<Option<bool>> {
                let delegate = $delegate;
                delegate.is_ancestor($ctx, ancestor, descendant).await
            }

            async fn build_up_to_heads(&$self, $ctx: &CoreContext, heads: &[ChangesetId]) -> Result<bool> {
                let delegate = $delegate;
                delegate.build_up_to_heads($ctx, heads).await
            }
        }
    };
}

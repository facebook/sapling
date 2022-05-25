/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! A store for Mercurial mutation information.
//!
//! This allows mutation information (i.e., which commits where amended or rebased) to be exchanged
//! between Mercurial clients via Mononoke.
//!
//! See <https://fburl.com/m2y3nr5c> for more details.

use std::collections::HashSet;

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use mercurial_types::HgChangesetId;

mod builder;
mod entry;
mod grouper;
mod store;

pub use crate::builder::SqlHgMutationStoreBuilder;
pub use crate::entry::HgMutationEntry;
pub use crate::store::SqlHgMutationStore;

#[facet::facet]
#[async_trait]
pub trait HgMutationStore: Send + Sync {
    /// Add new entries to the mutation store.
    ///
    /// Adds mutation information for `new_changeset_ids` using the given `entries`.
    async fn add_entries(
        &self,
        ctx: &CoreContext,
        new_changeset_ids: HashSet<HgChangesetId>,
        entries: Vec<HgMutationEntry>,
    ) -> Result<()>;

    /// Get all predecessor information for the given changeset ids.
    ///
    /// Returns all entries that describe the mutation history of the commits.
    async fn all_predecessors(
        &self,
        ctx: &CoreContext,
        changeset_ids: HashSet<HgChangesetId>,
    ) -> Result<Vec<HgMutationEntry>>;
}

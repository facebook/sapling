/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! A store for Mercurial mutation information.
//!
//! This allows mutation information (i.e., which commits where amended or rebased) to be exchanged
//! between Mercurial clients via Mononoke.
//!
//! See https://fburl.com/m2y3nr5c for more details.

#![deny(warnings)]

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use mercurial_types::HgChangesetId;

mod builder;
mod entry;
mod store;

pub use crate::builder::SqlHgMutationStoreBuilder;
pub use crate::entry::HgMutationEntry;
pub use crate::store::SqlHgMutationStore;

#[async_trait]
pub trait HgMutationStore: Send + Sync {
    /// Add new entries to the mutation store.
    ///
    /// Adds mutation information for `new_changeset_ids` using the given `entries`.
    async fn add_entries(
        &self,
        new_changeset_ids: HashSet<HgChangesetId>,
        entries: Vec<HgMutationEntry>,
    ) -> Result<()>;

    /// Get all predecessor information for the given changeset ids.
    ///
    /// Returns all entries that describe the mutation history of the commits.
    async fn all_predecessors(
        &self,
        changeset_ids: HashSet<HgChangesetId>,
    ) -> Result<Vec<HgMutationEntry>>;
}

#[async_trait]
impl HgMutationStore for Arc<dyn HgMutationStore> {
    async fn add_entries(
        &self,
        new_changeset_ids: HashSet<HgChangesetId>,
        entries: Vec<HgMutationEntry>,
    ) -> Result<()> {
        (**self).add_entries(new_changeset_ids, entries).await
    }

    async fn all_predecessors(
        &self,
        changeset_ids: HashSet<HgChangesetId>,
    ) -> Result<Vec<HgMutationEntry>> {
        (**self).all_predecessors(changeset_ids).await
    }
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Facet over the global `enabled_derived_data_types` metadata table.
//!
//! Presence of a `(repo_id, derived_data_type)` row means that derived data type
//! is enabled for that repo. This mirrors the `git_source_of_truth` facet: a
//! per-repo facet backed by a single global table that also exposes cross-repo
//! queries. No cache — writes are immediately visible to running services.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::DerivableType;
use mononoke_types::RepositoryId;

mod store;
mod types;

pub use crate::store::SqlEnabledDerivedDataTypes;
pub use crate::store::SqlEnabledDerivedDataTypesBuilder;
pub use crate::types::EnabledDerivedDataTypeEntry;
pub use crate::types::SqlDerivableType;

/// Enum representing the staleness of a read against the enabled-types table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Staleness {
    /// Read the most recent state (read from master).
    MostRecent,
    /// Best-effort recency (read from a replica).
    MaybeStale,
}

#[facet::facet]
#[async_trait]
pub trait EnabledDerivedDataTypes: Send + Sync {
    /// Idempotently record that `derived_data_type` is enabled for `repo_id`.
    ///
    /// If a row already exists it is left untouched (its `root_request_id` is
    /// preserved) — this is an `INSERT OR IGNORE` / `ON DUPLICATE KEY UPDATE`
    /// no-op, never a clobber.
    async fn mark_enabled(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        derived_data_type: DerivableType,
        root_request_id: Option<u64>,
    ) -> Result<()>;

    /// Return the set of derived data types enabled for `repo_id`.
    async fn get_enabled_types(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        staleness: Staleness,
    ) -> Result<Vec<DerivableType>>;

    /// Return every enabled-type entry across all repos. Used by the reconciler.
    async fn get_all(&self, ctx: &CoreContext) -> Result<Vec<EnabledDerivedDataTypeEntry>>;
}

/// In-memory implementation of [`EnabledDerivedDataTypes`] for unit tests.
#[derive(Clone, Default)]
pub struct TestEnabledDerivedDataTypes {
    // Keyed by the natural (repo_id, derived_data_type) primary key.
    entries: Arc<Mutex<HashMap<(RepositoryId, DerivableType), EnabledDerivedDataTypeEntry>>>,
}

impl TestEnabledDerivedDataTypes {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl EnabledDerivedDataTypes for TestEnabledDerivedDataTypes {
    async fn mark_enabled(
        &self,
        _ctx: &CoreContext,
        repo_id: RepositoryId,
        derived_data_type: DerivableType,
        root_request_id: Option<u64>,
    ) -> Result<()> {
        let mut map = self.entries.lock().expect("poisoned lock");
        // INSERT OR IGNORE semantics: keep the existing row if present.
        map.entry((repo_id, derived_data_type))
            .or_insert(EnabledDerivedDataTypeEntry {
                repo_id,
                derived_data_type,
                root_request_id,
            });
        Ok(())
    }

    async fn get_enabled_types(
        &self,
        _ctx: &CoreContext,
        repo_id: RepositoryId,
        _staleness: Staleness,
    ) -> Result<Vec<DerivableType>> {
        Ok(self
            .entries
            .lock()
            .expect("poisoned lock")
            .values()
            .filter(|entry| entry.repo_id == repo_id)
            .map(|entry| entry.derived_data_type)
            .collect())
    }

    async fn get_all(&self, _ctx: &CoreContext) -> Result<Vec<EnabledDerivedDataTypeEntry>> {
        Ok(self
            .entries
            .lock()
            .expect("poisoned lock")
            .values()
            .cloned()
            .collect())
    }
}

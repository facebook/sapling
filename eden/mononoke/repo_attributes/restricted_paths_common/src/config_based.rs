/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Config-based restricted paths lookup.
//!
//! Provides synchronous, config-driven restricted path checks that do not
//! require derived data or async context.

use std::sync::Arc;

use metaconfig_types::RestrictedPathsConfig;
use mononoke_types::NonRootMPath;
use permission_checker::MononokeIdentity;

use crate::cache::RestrictedPathsManifestIdCache;
use crate::manifest_id_store::ArcRestrictedPathsManifestIdStore;

/// Repository restricted paths configuration — config-based lookups only.
///
/// This facet holds everything that derived-data crates need without pulling
/// in the full `RestrictedPaths` facet (which depends on `repo_derived_data`).
#[facet::facet]
pub struct RestrictedPathsConfigBased {
    config: RestrictedPathsConfig,
    manifest_id_store: ArcRestrictedPathsManifestIdStore,
    manifest_id_cache: Option<Arc<RestrictedPathsManifestIdCache>>,
}

impl RestrictedPathsConfigBased {
    pub fn new(
        config: RestrictedPathsConfig,
        manifest_id_store: ArcRestrictedPathsManifestIdStore,
        manifest_id_cache: Option<Arc<RestrictedPathsManifestIdCache>>,
    ) -> Self {
        Self {
            config,
            manifest_id_store,
            manifest_id_cache,
        }
    }

    pub fn config(&self) -> &RestrictedPathsConfig {
        &self.config
    }

    pub fn manifest_id_store(&self) -> &ArcRestrictedPathsManifestIdStore {
        &self.manifest_id_store
    }

    pub fn manifest_id_cache(&self) -> Option<&Arc<RestrictedPathsManifestIdCache>> {
        self.manifest_id_cache.as_ref()
    }

    /// Returns whether any restricted paths are configured for this repository.
    pub fn has_restricted_paths(&self) -> bool {
        !self.config.path_acls.is_empty()
    }

    /// Check if a path is itself a restriction root (exact match).
    /// Returns false for paths that are merely under a restriction root.
    pub fn is_restriction_root(&self, path: &NonRootMPath) -> bool {
        self.get_acl_for_path(path).is_some()
    }

    /// Exact path match against config.path_acls.
    pub fn get_acl_for_path(&self, path: &NonRootMPath) -> Option<&MononokeIdentity> {
        self.config
            .path_acls
            .iter()
            .find(|(restricted_path_prefix, _)| *restricted_path_prefix == path)
            .map(|(_, acl)| acl)
    }

    /// Prefix match against config.path_acls.
    /// If `foo` is under ACL X, calling this with `foo/bar` will return `X`.
    pub fn get_acl_for_path_prefix(&self, path: &NonRootMPath) -> Option<&MononokeIdentity> {
        // TODO(T239041722): use SortedVectorMap to ensure a specific order
        self.config
            .path_acls
            .iter()
            .find(|(restricted_path_prefix, _)| restricted_path_prefix.is_prefix_of(path))
            .map(|(_, acl)| acl)
    }
}

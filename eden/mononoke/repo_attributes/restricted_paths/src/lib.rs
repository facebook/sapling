/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Restricted Paths.
//!
//! Abstractions to track a repo's restricted paths, along with their ACLs,
//! and to store the manifest ids of these paths from every revision.

mod access_log;
mod cache;
mod manifest_id_store;

use std::sync::Arc;

use anyhow::Result;
use context::CoreContext;
use metaconfig_types::RestrictedPathsConfig;
use mononoke_types::NonRootMPath;
use permission_checker::AclProvider;
use permission_checker::MononokeIdentity;
use scuba_ext::MononokeScubaSampleBuilder;

pub use crate::access_log::ACCESS_LOG_SCUBA_TABLE;
use crate::access_log::log_access_to_restricted_path;
pub use crate::cache::ManifestIdCache;
pub use crate::cache::RestrictedPathsManifestIdCache;
pub use crate::cache::RestrictedPathsManifestIdCacheBuilder;
pub use crate::manifest_id_store::ArcRestrictedPathsManifestIdStore;
pub use crate::manifest_id_store::ManifestId;
pub use crate::manifest_id_store::ManifestType;
pub use crate::manifest_id_store::RestrictedPathManifestIdEntry;
pub use crate::manifest_id_store::RestrictedPathsManifestIdStore;
pub use crate::manifest_id_store::SqlRestrictedPathsManifestIdStore;
pub use crate::manifest_id_store::SqlRestrictedPathsManifestIdStoreBuilder;

/// Repository restricted paths configuration.
#[facet::facet]
pub struct RestrictedPaths {
    /// The restricted paths configuration for this repo, i.e. the restricted
    /// paths and their associated ACLs.
    config: RestrictedPathsConfig,
    /// Storage for the manifest ids of the restricted paths.
    manifest_id_store: ArcRestrictedPathsManifestIdStore,
    /// ACL provider for authorization checks
    acl_provider: Arc<dyn AclProvider>,
    /// Optional in-memory cache for manifest ID lookups, instead of direct DB
    /// queries
    manifest_id_cache: Option<Arc<RestrictedPathsManifestIdCache>>,
    /// Scuba sample builder for logging access to restricted paths
    scuba: MononokeScubaSampleBuilder,
}

impl RestrictedPaths {
    pub fn new(
        config: RestrictedPathsConfig,
        manifest_id_store: Arc<dyn RestrictedPathsManifestIdStore>,
        acl_provider: Arc<dyn AclProvider>,
        manifest_id_cache: Option<Arc<RestrictedPathsManifestIdCache>>,
        scuba: MononokeScubaSampleBuilder,
    ) -> Self {
        Self {
            config,
            manifest_id_store,
            acl_provider,
            manifest_id_cache,
            scuba,
        }
    }

    pub fn config(&self) -> &RestrictedPathsConfig {
        &self.config
    }

    pub fn manifest_id_store(&self) -> &ArcRestrictedPathsManifestIdStore {
        &self.manifest_id_store
    }

    /// If a path is considered restricted according to the configuration,
    /// returns its associated ACL.
    /// This will **NOT consider child directories** as restricted. e.g.
    /// If `foo` is under ACL X, calling this `foo/bar` will return None.
    pub fn get_acl_for_path(&self, path: &NonRootMPath) -> Option<&MononokeIdentity> {
        let config = &self.config;

        // Check if the path starts with any of the configured restricted path prefixes
        for (restricted_path_prefix, acl) in &config.path_acls {
            if restricted_path_prefix == path {
                return Some(acl);
            }
        }

        None
    }

    pub fn get_acls_for_paths(&self, paths: &[NonRootMPath]) -> Vec<&MononokeIdentity> {
        paths
            .iter()
            .filter_map(|path| self.get_acl_for_path(path))
            .collect()
    }

    /// If the **exact** path is considered restricted according to the
    /// configuration, returns its associated ACL.
    /// This will **consider child directories** as restricted. e.g.
    /// If `foo` is under ACL X, calling this `foo/bar` will return `X`.
    pub fn get_acl_for_path_prefix(&self, path: &NonRootMPath) -> Option<&MononokeIdentity> {
        let config = &self.config;

        // TODO(T239041722): use SortedVectorMap to ensure a specific order

        // Check if the path starts with any of the configured restricted path prefixes
        for (restricted_path_prefix, acl) in &config.path_acls {
            if restricted_path_prefix.is_prefix_of(path) {
                return Some(acl);
            }
        }

        None
    }

    /// Check if a path is considered restricted according to the configuration.
    /// This will not consider children as restricted, i.e. it's a strict comparison.
    pub fn is_restricted_path(&self, path: &NonRootMPath) -> bool {
        self.get_acl_for_path(path).is_some()
    }

    /// Check if any restricted paths are configured for this repository.
    pub fn has_restricted_paths(&self) -> bool {
        !self.config.path_acls.is_empty()
    }

    /// Check if a manifest id belongs to a restricted path and log access it it.
    ///
    /// Returns true if caller is authorized to access it.
    pub async fn log_access_by_manifest_if_restricted(
        &self,
        ctx: &CoreContext,
        manifest_id: ManifestId,
        manifest_type: ManifestType,
    ) -> Result<bool> {
        // No need to query the DB if the config is empty, i.e. the repo doesn't
        // have any restricted paths.

        if self.config().is_empty() {
            return Ok(true);
        }

        // Try to use cache first, fall back to DB query if cache is not available
        let paths = if let Some(manifest_id_cache) = &self.manifest_id_cache {
            // Read from cache
            let cache_guard = manifest_id_cache.cache().read().unwrap();
            cache_guard
                .get(&manifest_type)
                .and_then(|type_map| type_map.get(&manifest_id))
                .cloned()
                .unwrap_or_default()
        } else {
            // Fall back to DB query if cache is not available
            self.manifest_id_store
                .get_paths_by_manifest_id(ctx, &manifest_id, &manifest_type)
                .await?
        };

        if paths.is_empty() {
            return Ok(true);
        }

        let acls = self.get_acls_for_paths(paths.as_slice());

        log_access_to_restricted_path(
            ctx,
            self.manifest_id_store.repo_id(),
            paths,
            acls,
            crate::access_log::RestrictedPathAccessData::Manifest(manifest_id, manifest_type),
            self.acl_provider.clone(),
            self.scuba.clone(),
        )
        .await
    }

    /// Log access to a restricted path, when it's accessed by the full path,
    /// instead of a manifest id.
    ///
    /// Returns true if caller is authorized to access it.
    pub async fn log_access_by_path_if_restricted(
        &self,
        ctx: &CoreContext,
        path: NonRootMPath,
    ) -> Result<bool> {
        // Return early if the repo doesn't have any restricted paths.
        if self.config().is_empty() {
            return Ok(true);
        }

        // Find which restricted path roots match this path
        let mut restricted_path_roots = Vec::new();
        let mut matched_acls = Vec::new();

        for (restricted_path_prefix, acl) in &self.config.path_acls {
            if restricted_path_prefix.is_prefix_of(&path) {
                restricted_path_roots.push(restricted_path_prefix.clone());
                matched_acls.push(acl);
            }
        }

        // If no restricted paths match, no need to log
        if restricted_path_roots.is_empty() {
            return Ok(true);
        }

        log_access_to_restricted_path(
            ctx,
            self.manifest_id_store.repo_id(),
            restricted_path_roots,
            matched_acls,
            crate::access_log::RestrictedPathAccessData::FullPath { full_path: path },
            self.acl_provider.clone(),
            self.scuba.clone(),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::Arc;

    use anyhow::Result;
    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;
    use mononoke_types::NonRootMPath;
    use mononoke_types::RepositoryId;
    use permission_checker::dummy::DummyAclProvider;
    use sql_construct::SqlConstruct;

    use super::*;
    use crate::SqlRestrictedPathsManifestIdStoreBuilder;

    #[mononoke::fbinit_test]
    fn test_empty_config(fb: FacebookInit) -> Result<()> {
        let repo_id = RepositoryId::new(0);

        let acl_provider = DummyAclProvider::new(fb)?;
        let manifest_id_store = Arc::new(
            SqlRestrictedPathsManifestIdStoreBuilder::with_sqlite_in_memory()
                .expect("Failed to create Sqlite connection")
                .with_repo_id(repo_id),
        );

        let scuba = MononokeScubaSampleBuilder::with_discard();

        let repo_restricted_paths = RestrictedPaths::new(
            RestrictedPathsConfig::default(),
            manifest_id_store,
            acl_provider,
            None,
            scuba,
        );

        assert!(!repo_restricted_paths.has_restricted_paths());

        let test_path = NonRootMPath::new("test/path").unwrap();
        assert!(repo_restricted_paths.get_acl_for_path(&test_path).is_none());

        Ok(())
    }

    #[mononoke::fbinit_test]
    fn test_with_config(fb: FacebookInit) -> Result<()> {
        let repo_id = RepositoryId::new(0);
        let mut path_acls = HashMap::new();
        let use_manifest_id_cache = true;
        let cache_update_interval_ms = 100;

        let acl_provider = DummyAclProvider::new(fb)?;
        path_acls.insert(
            NonRootMPath::new("restricted/dir").unwrap(),
            MononokeIdentity::from_str("SERVICE_IDENTITY:restricted_acl")?,
        );
        path_acls.insert(
            NonRootMPath::new("other/restricted").unwrap(),
            MononokeIdentity::from_str("SERVICE_IDENTITY:other_acl")?,
        );

        let manifest_id_store = Arc::new(
            SqlRestrictedPathsManifestIdStoreBuilder::with_sqlite_in_memory()
                .expect("Failed to create Sqlite connection")
                .with_repo_id(repo_id),
        );
        let config = RestrictedPathsConfig {
            path_acls,
            use_manifest_id_cache,
            cache_update_interval_ms,
        };

        let scuba = MononokeScubaSampleBuilder::with_discard();

        let repo_restricted_paths =
            RestrictedPaths::new(config, manifest_id_store, acl_provider, None, scuba);

        assert!(repo_restricted_paths.has_restricted_paths());
        Ok(())
    }

    #[mononoke::fbinit_test]
    fn test_path_matching(fb: FacebookInit) -> Result<()> {
        let repo_id = RepositoryId::new(0);
        let mut path_acls = HashMap::new();
        let use_manifest_id_cache = true;
        let cache_update_interval_ms = 100;

        let acl_provider = DummyAclProvider::new(fb)?;
        let restricted_acl = MononokeIdentity::from_str("SERVICE_IDENTITY:restricted_acl")?;
        path_acls.insert(
            NonRootMPath::new("restricted/dir").unwrap(),
            restricted_acl.clone(),
        );

        let manifest_id_store = Arc::new(
            SqlRestrictedPathsManifestIdStoreBuilder::with_sqlite_in_memory()
                .expect("Failed to create Sqlite connection")
                .with_repo_id(repo_id),
        );

        let config = RestrictedPathsConfig {
            path_acls,
            use_manifest_id_cache,
            cache_update_interval_ms,
        };

        let scuba = MononokeScubaSampleBuilder::with_discard();

        let repo_restricted_paths =
            RestrictedPaths::new(config, manifest_id_store, acl_provider, None, scuba);

        // Test exact match
        let exact_path = NonRootMPath::new("restricted/dir").unwrap();
        assert_eq!(
            repo_restricted_paths.get_acl_for_path(&exact_path),
            Some(&restricted_acl)
        );

        // Test subdirectory match
        let sub_path = NonRootMPath::new("restricted/dir/subdir/file.txt").unwrap();
        assert!(repo_restricted_paths.get_acl_for_path(&sub_path).is_none());

        // Test non-matching path
        let other_path = NonRootMPath::new("other/dir/file.txt").unwrap();
        assert!(
            repo_restricted_paths
                .get_acl_for_path(&other_path)
                .is_none()
        );

        // Test path that shared parent directory. Should not match.
        let partial_path = NonRootMPath::new("restricted/different").unwrap();
        assert!(
            repo_restricted_paths
                .get_acl_for_path(&partial_path)
                .is_none()
        );

        // Test partial match, differring by on character. Should not match
        let partial_path = NonRootMPath::new("restricted/di").unwrap();
        assert!(
            repo_restricted_paths
                .get_acl_for_path(&partial_path)
                .is_none()
        );
        Ok(())
    }

    // TODO(T239041722): test nested paths with different ACLs. Should we use SortedVectorMap??
}

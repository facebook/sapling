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

mod manifest_id_store;

use std::sync::Arc;

use metaconfig_types::RestrictedPathsConfig;
use mononoke_types::NonRootMPath;
use permission_checker::MononokeIdentity;

pub use crate::manifest_id_store::ArcRestrictedPathsManifestIdStore;
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
}

impl RestrictedPaths {
    pub fn new(
        config: RestrictedPathsConfig,
        manifest_id_store: Arc<dyn RestrictedPathsManifestIdStore>,
    ) -> Self {
        Self {
            config,
            manifest_id_store,
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
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::str::FromStr;

    use anyhow::Result;
    use mononoke_macros::mononoke;
    use mononoke_types::NonRootMPath;
    use mononoke_types::RepositoryId;
    use sql_construct::SqlConstruct;

    use super::*;
    use crate::SqlRestrictedPathsManifestIdStoreBuilder;

    #[mononoke::test]
    fn test_empty_config() {
        let repo_id = RepositoryId::new(0);
        let manifest_id_store = Arc::new(
            SqlRestrictedPathsManifestIdStoreBuilder::with_sqlite_in_memory()
                .expect("Failed to create Sqlite connection")
                .with_repo_id(repo_id),
        );
        let repo_restricted_paths =
            RestrictedPaths::new(RestrictedPathsConfig::default(), manifest_id_store);

        assert!(!repo_restricted_paths.has_restricted_paths());

        let test_path = NonRootMPath::new("test/path").unwrap();
        assert!(repo_restricted_paths.get_acl_for_path(&test_path).is_none());
    }

    #[mononoke::test]
    fn test_with_config() -> Result<()> {
        let repo_id = RepositoryId::new(0);
        let mut path_acls = HashMap::new();
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
        let config = RestrictedPathsConfig { path_acls };
        let repo_restricted_paths = RestrictedPaths::new(config, manifest_id_store);

        assert!(repo_restricted_paths.has_restricted_paths());
        Ok(())
    }

    #[mononoke::test]
    fn test_path_matching() -> Result<()> {
        let repo_id = RepositoryId::new(0);
        let mut path_acls = HashMap::new();
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

        let config = RestrictedPathsConfig { path_acls };
        let repo_restricted_paths = RestrictedPaths::new(config, manifest_id_store);

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

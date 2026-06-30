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

use metaconfig_types::PathRestrictionMetadata;
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
        !self.config.path_restriction_metadata.is_empty()
    }

    /// Check if a path is itself a restriction root (exact match).
    /// Returns false for paths that are merely under a restriction root.
    pub fn is_restriction_root(&self, path: &NonRootMPath) -> bool {
        self.get_metadata_for_path(path).is_some()
    }

    /// Exact path match against the configured restriction metadata.
    pub fn get_metadata_for_path(&self, path: &NonRootMPath) -> Option<&PathRestrictionMetadata> {
        self.config.path_restriction_metadata.get(path)
    }

    /// REPO_REGION ACL for an exact restriction-root path.
    pub fn get_acl_for_path(&self, path: &NonRootMPath) -> Option<&MononokeIdentity> {
        self.get_metadata_for_path(path)
            .map(|metadata| &metadata.repo_region_acl)
    }

    /// Prefix match against the configured restriction metadata.
    /// If `foo` is under ACL X, calling this with `foo/bar` will return `X`.
    pub fn get_acl_for_path_prefix(&self, path: &NonRootMPath) -> Option<&MononokeIdentity> {
        // TODO(T239041722): use SortedVectorMap to ensure a specific order
        self.config
            .path_restriction_metadata
            .iter()
            .find(|(restricted_path_prefix, _)| restricted_path_prefix.is_prefix_of(path))
            .map(|(_, metadata)| &metadata.repo_region_acl)
    }

    /// Whether derivation should record a new manifest-id-store entry for this
    /// path: true only when the path is a restriction root AND not read-only.
    /// Distinct from `is_restriction_root` (which stays read-only-agnostic) so
    /// that enforcement on already-recorded entries is never weakened.
    pub fn should_record_manifest_id_entry(&self, path: &NonRootMPath) -> bool {
        self.get_metadata_for_path(path)
            .is_some_and(|metadata| !metadata.read_only)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::Arc;

    use anyhow::Result;
    use mononoke_macros::mononoke;
    use mononoke_types::RepositoryId;
    use sql_construct::SqlConstruct;

    use super::*;
    use crate::manifest_id_store::SqlRestrictedPathsManifestIdStoreBuilder;

    fn metadata(acl: &str, read_only: bool) -> PathRestrictionMetadata {
        PathRestrictionMetadata {
            repo_region_acl: MononokeIdentity::from_str(acl).expect("valid identity"),
            permission_request_group: None,
            read_only,
        }
    }

    fn config_based_with_metadata(
        entries: Vec<(&str, PathRestrictionMetadata)>,
    ) -> Result<RestrictedPathsConfigBased> {
        let path_restriction_metadata = entries
            .into_iter()
            .map(|(path, md)| Ok((NonRootMPath::new(path)?, md)))
            .collect::<Result<_>>()?;
        let store = Arc::new(
            SqlRestrictedPathsManifestIdStoreBuilder::with_sqlite_in_memory()?
                .with_repo_id(RepositoryId::new(0)),
        );
        Ok(RestrictedPathsConfigBased::new(
            RestrictedPathsConfig {
                path_restriction_metadata,
                ..Default::default()
            },
            store,
            None,
        ))
    }

    /// What it tests: the derivation recording gate honors `read_only`.
    /// Expected: a writable root records; a read-only root does not; a
    /// non-restriction path does not. `is_restriction_root` stays
    /// read-only-agnostic so enforcement on existing entries is never weakened.
    #[mononoke::test]
    fn test_should_record_manifest_id_entry_respects_read_only() -> Result<()> {
        let config_based = config_based_with_metadata(vec![
            ("writable", metadata("REPO_REGION:acl1", false)),
            ("frozen", metadata("REPO_REGION:acl2", true)),
        ])?;

        let writable = NonRootMPath::new("writable")?;
        let frozen = NonRootMPath::new("frozen")?;
        let other = NonRootMPath::new("other")?;

        assert!(config_based.should_record_manifest_id_entry(&writable));
        assert!(config_based.is_restriction_root(&writable));

        assert!(
            !config_based.should_record_manifest_id_entry(&frozen),
            "read_only roots must not record new manifest-id entries"
        );
        assert!(
            config_based.is_restriction_root(&frozen),
            "read_only roots are still restriction roots"
        );

        assert!(!config_based.should_record_manifest_id_entry(&other));
        assert!(!config_based.is_restriction_root(&other));

        Ok(())
    }
}

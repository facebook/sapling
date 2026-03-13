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

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use context::CoreContext;
use metaconfig_types::RestrictedPathsConfig;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
use permission_checker::AclProvider;
use permission_checker::MononokeIdentity;
use repo_derived_data::ArcRepoDerivedData;
pub use restricted_paths_common::*;
use scuba_ext::MononokeScubaSampleBuilder;
use thiserror::Error;
use tokio::task;

pub use crate::access_log::ACCESS_LOG_SCUBA_TABLE;
pub use crate::access_log::has_read_access_to_repo_region_acls;
use crate::access_log::is_member_of_groups;
use crate::access_log::log_access_to_restricted_path;
/// Core restriction information for a path.
/// Does not include access check results — that is the API layer's concern
/// (see `mononoke_api::PathAccessInfo`).
#[derive(Clone, Debug, PartialEq)]
pub struct PathRestrictionInfo {
    /// The root path of this restriction (directory containing `.slacl`).
    pub restriction_root: NonRootMPath,
    /// The repo region ACL string, e.g. "REPO_REGION:repos/hg/fbsource/=project1".
    pub repo_region_acl: String,
    /// ACL for requesting access. Defaults to repo_region_acl if not configured.
    pub request_acl: String,
}

#[derive(Debug)]
pub enum RestrictedPathAccessType<'a> {
    Manifest(ManifestId),
    Path(&'a MPath),
}

/// Error type for restricted paths enforcement.
#[derive(Debug, Error)]
pub enum RestrictedPathsError<'a> {
    #[error("Access denied: unauthorized access to restricted path: {0}")]
    AuthorizationError(RestrictedPathAccessType<'a>),
    #[error("Internal error: {0}")]
    InternalError(#[from] anyhow::Error),
}

impl<'a> std::fmt::Display for RestrictedPathAccessType<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manifest(manifest_id) => {
                write!(f, "ManifestId({})", manifest_id)
            }
            Self::Path(path) => write!(f, "{}", path),
        }
    }
}

/// Repository restricted paths configuration.
#[facet::facet]
pub struct RestrictedPaths {
    /// Config-based restricted paths (shared with derived-data crates).
    config_based: Arc<RestrictedPathsConfigBased>,
    /// ACL provider for authorization checks
    acl_provider: Arc<dyn AclProvider>,
    /// Scuba sample builder for logging access to restricted paths
    scuba: MononokeScubaSampleBuilder,
    /// Whether to use ACL manifest instead of config for restriction lookups.
    use_acl_manifest: bool,
    /// Repo derived data for deriving ACL manifests.
    /// Used by the manifest-based lookup methods added in a follow-up commit.
    #[allow(dead_code)]
    repo_derived_data: ArcRepoDerivedData,
}

impl RestrictedPaths {
    pub fn new(
        config_based: Arc<RestrictedPathsConfigBased>,
        acl_provider: Arc<dyn AclProvider>,
        scuba: MononokeScubaSampleBuilder,
        use_acl_manifest: bool,
        repo_derived_data: ArcRepoDerivedData,
    ) -> Result<Self> {
        if use_acl_manifest {
            anyhow::ensure!(
                repo_derived_data
                    .config()
                    .is_enabled(DerivableType::AclManifests),
                "use_acl_manifest is true but AclManifest derivation is not enabled for this repo. \
                 Enable AclManifests in the repo's derived data config."
            );
        }
        Ok(Self {
            config_based,
            acl_provider,
            scuba,
            use_acl_manifest,
            repo_derived_data,
        })
    }

    // TODO(T248660053): make pub(crate) once hooks use dedicated primitives
    // instead of accessing path_acls directly. Blocked on adding a primitive
    // for "is path related to any restriction root" (used by block_restricted_copy
    // and block_restricted_subtree_copy hooks).
    pub fn config(&self) -> &RestrictedPathsConfig {
        self.config_based.config()
    }

    /// Returns whether any restricted paths are configured for this repository.
    pub fn has_restricted_paths(&self) -> bool {
        self.config_based.has_restricted_paths()
    }

    /// Returns the soft path ACLs configuration.
    pub fn soft_path_acls(&self) -> &[metaconfig_types::SoftRestrictedPathConfig] {
        &self.config_based.config().soft_path_acls
    }

    /// Returns the underlying config-based restricted paths.
    pub fn config_based(&self) -> &Arc<RestrictedPathsConfigBased> {
        &self.config_based
    }

    pub fn acl_provider(&self) -> &Arc<dyn AclProvider> {
        &self.acl_provider
    }

    /// Returns whether ACL manifest should be used for restriction lookups.
    pub fn use_acl_manifest(&self) -> bool {
        self.use_acl_manifest
    }

    // -----------------------------------------------------------------------
    // Public restriction lookup methods
    // -----------------------------------------------------------------------

    /// Get exact path restriction info for one or more paths.
    /// Does NOT consider parent directories — only exact matches.
    pub async fn get_exact_path_restriction(
        &self,
        _ctx: &CoreContext,
        _cs_id: Option<ChangesetId>,
        paths: &[NonRootMPath],
    ) -> Result<Vec<PathRestrictionInfo>> {
        // TODO(T248660053): when use_acl_manifest is true, use manifest-based lookup
        Ok(paths
            .iter()
            .filter_map(|path| {
                self.config_based.get_acl_for_path(path).map(|acl| {
                    let repo_region_acl = acl.to_string();
                    PathRestrictionInfo {
                        restriction_root: path.clone(),
                        request_acl: repo_region_acl.clone(),
                        repo_region_acl,
                    }
                })
            })
            .collect())
    }

    /// Get restriction info for one or more paths, considering ancestor restrictions.
    /// For each path, collects restrictions at every ancestor directory.
    pub async fn get_path_restriction_info(
        &self,
        _ctx: &CoreContext,
        _cs_id: Option<ChangesetId>,
        paths: &[NonRootMPath],
    ) -> Result<Vec<PathRestrictionInfo>> {
        // TODO(T248660053): when use_acl_manifest is true, use manifest-based lookup
        Ok(paths
            .iter()
            .flat_map(|path| {
                self.config()
                    .path_acls
                    .iter()
                    .filter(|(prefix, _)| prefix.is_prefix_of(path))
                    .map(|(prefix, acl)| {
                        let repo_region_acl = acl.to_string();
                        PathRestrictionInfo {
                            restriction_root: prefix.clone(),
                            request_acl: repo_region_acl.clone(),
                            repo_region_acl,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect())
    }

    /// Check if a path is itself a restriction root (exact match).
    /// Returns false for paths that are merely under a restriction root.
    pub async fn is_restriction_root(
        &self,
        ctx: &CoreContext,
        cs_id: Option<ChangesetId>,
        path: &NonRootMPath,
    ) -> Result<bool> {
        self.get_exact_path_restriction(ctx, cs_id, std::slice::from_ref(path))
            .await
            .map(|r| !r.is_empty())
    }

    /// Check if a path is restricted, considering ancestor directories.
    /// Returns true if the path itself or any of its ancestors is a restriction root.
    pub async fn is_restricted_path(
        &self,
        ctx: &CoreContext,
        cs_id: Option<ChangesetId>,
        path: &NonRootMPath,
    ) -> Result<bool> {
        self.get_path_restriction_info(ctx, cs_id, std::slice::from_ref(path))
            .await
            .map(|r| !r.is_empty())
    }

    /// Find all restriction roots that are descendants of any of the given root paths.
    /// Results are deduplicated by restriction_root.
    pub async fn find_restricted_descendants(
        &self,
        _ctx: &CoreContext,
        _cs_id: Option<ChangesetId>,
        roots: Vec<MPath>,
    ) -> Result<Vec<PathRestrictionInfo>> {
        // TODO(T248660053): when use_acl_manifest is true, use manifest-based lookup
        let mut results: Vec<PathRestrictionInfo> = self
            .config()
            .path_acls
            .iter()
            .filter(|(root, _)| {
                roots
                    .iter()
                    .any(|query_root| query_root.is_root() || query_root.is_prefix_of(*root))
            })
            .map(|(root, acl)| {
                let repo_region_acl = acl.to_string();
                PathRestrictionInfo {
                    restriction_root: root.clone(),
                    request_acl: repo_region_acl.clone(),
                    repo_region_acl,
                }
            })
            .collect();
        results.sort_by(|a, b| a.restriction_root.cmp(&b.restriction_root));
        results.dedup_by(|a, b| a.restriction_root == b.restriction_root);
        Ok(results)
    }

    // -----------------------------------------------------------------------
    // Public access logging methods
    // -----------------------------------------------------------------------

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
        let paths = if let Some(manifest_id_cache) = self.config_based.manifest_id_cache() {
            // Read from cache
            let cache_guard = manifest_id_cache
                .cache()
                .read()
                .map_err(|e| anyhow::anyhow!("Failed to acquire cache read lock: {}", e))?;
            cache_guard
                .get(&manifest_type)
                .and_then(|type_map| type_map.get(&manifest_id))
                .cloned()
                .unwrap_or_default()
        } else {
            // Fall back to DB query if cache is not available
            self.config_based
                .manifest_id_store()
                .get_paths_by_manifest_id(ctx, &manifest_id, &manifest_type)
                .await?
        };

        if paths.is_empty() {
            return Ok(true);
        }

        // Use config-based lookup directly — this method works with manifest IDs
        // from the restricted paths store, not with changesets, so we always use
        // the config to determine which paths are restricted.
        // TODO(T248660053): support manifest-based access usign AclManifests.
        let acls: Vec<&MononokeIdentity> = paths
            .iter()
            .filter_map(|path| self.config_based.get_acl_for_path(path))
            .collect();

        log_access_to_restricted_path(
            ctx,
            self.config_based.manifest_id_store().repo_id(),
            paths,
            acls,
            crate::access_log::RestrictedPathAccessData::Manifest(manifest_id, manifest_type),
            self.acl_provider.clone(),
            self.config().tooling_allowlist_group.as_deref(),
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
        let (restricted_path_roots, matched_acls): (Vec<_>, Vec<_>) = self
            .config()
            .path_acls
            .iter()
            .filter(|(restricted_path_prefix, _)| restricted_path_prefix.is_prefix_of(&path))
            .map(|(prefix, acl)| (prefix.clone(), acl))
            .unzip();

        // If no restricted paths match, no need to log
        if restricted_path_roots.is_empty() {
            return Ok(true);
        }

        log_access_to_restricted_path(
            ctx,
            self.config_based.manifest_id_store().repo_id(),
            restricted_path_roots,
            matched_acls,
            crate::access_log::RestrictedPathAccessData::FullPath { full_path: path },
            self.acl_provider.clone(),
            self.config().tooling_allowlist_group.as_deref(),
            self.scuba.clone(),
        )
        .await
    }

    /// Check if any client identity matches the enforcement conditions config.
    /// Returns true if enforcement should be applied.
    async fn should_enforce_restriction(&self, ctx: &CoreContext) -> Result<bool> {
        let enforcement_acls = self.config().conditional_enforcement_acls.clone();
        if enforcement_acls.is_empty() {
            return Ok(false);
        }

        let acls: Vec<_> = enforcement_acls.iter().collect();
        is_member_of_groups(ctx, &self.acl_provider, acls.as_slice()).await
    }
}

/// Helper function to spawn an async task that logs access to restricted paths.
///
/// This function checks if restricted paths access logging is enabled via justknobs,
/// and if so, spawns an async task to log the access. The logging is done asynchronously
/// to avoid blocking the request.
///
/// Only spawns a task when scuba logging is actually enabled (not a discard builder).
/// This avoids unnecessary task spawning overhead when logging is disabled.
///
/// # Arguments
/// * `ctx` - The core context for the operation
/// * `restricted_paths` - Arc to the RestrictedPaths configuration
/// * `path` - The path being accessed (as an MPath)
/// * `switch_value` - The justknobs switch value to use for feature gating
///
/// # Returns
/// Ok(()) if the justknobs check succeeds, Err otherwise
pub fn spawn_log_restricted_path_access(
    ctx: &CoreContext,
    restricted_paths: Arc<RestrictedPaths>,
    path: &mononoke_types::MPath,
    switch_value: &str,
) -> Result<Option<task::JoinHandle<Result<bool>>>> {
    // Early return if logging is disabled - avoid all overhead
    if !justknobs::eval(
        "scm/mononoke:enabled_restricted_paths_access_logging",
        None,
        Some(switch_value),
    )? {
        return Ok(None);
    }

    // Early return if config is empty - no restricted paths to check
    if restricted_paths.config().is_empty() {
        return Ok(None);
    }

    // Only spawn task if we're actually going to log something
    if let Ok(non_root_mpath) = NonRootMPath::try_from(path.clone()) {
        let ctx_clone = ctx.clone();

        // Log asynchronously to avoid blocking the request
        let spawned_task = mononoke::spawn_task(async move {
            restricted_paths
                .log_access_by_path_if_restricted(&ctx_clone, non_root_mpath)
                .await
        });

        // But return the task handle so callers can wait on the access check result
        // if needed.
        return Ok(Some(spawned_task));
    }

    Ok(None)
}

/// Spawn enforcement check for restricted path access.
///
/// This function:
/// 1. Calls `spawn_log_restricted_path_access` for logging (fire-and-forget)
/// 2. Checks if enforcement JK is enabled
/// 3. Checks if any of the client identities match the condition enforcement ACLs
/// 4. If match AND user lacks authorization, returns `RestrictedPathsError::AuthorizationError`
///
/// # Returns
/// * `Ok(())` if access is allowed or enforcement is disabled
/// * `Err(RestrictedPathsError::AuthorizationError)` if access is denied
pub async fn spawn_enforce_restricted_path_access<'a, 'b>(
    ctx: &'b CoreContext,
    restricted_paths: Arc<RestrictedPaths>,
    path: &'a MPath,
    switch_value: &'b str,
) -> Result<(), RestrictedPathsError<'a>> {
    // Always log first, but get the task handle so we can get the access check
    // result if needed.
    let has_auth_task =
        spawn_log_restricted_path_access(ctx, restricted_paths.clone(), path, switch_value)?;

    // Check if enforcement JK is enabled
    let enforcement_enabled = justknobs::eval(
        "scm/mononoke:enable_server_side_path_acls",
        None,
        Some(switch_value),
    )?;

    // Early return if enforcement is disabled or there are no conditional
    // enforcement ACLs configured
    if !enforcement_enabled
        || restricted_paths
            .config()
            .conditional_enforcement_acls
            .is_empty()
    {
        return Ok(());
    }

    let should_enforce_restrictions = restricted_paths
        .should_enforce_restriction(ctx)
        .await
        .context("Checking if conditional enforcement ACLs match")?;

    if !should_enforce_restrictions {
        return Ok(());
    }

    // Conditional enforcement matched - check authorization
    let has_auth = if let Some(has_auth_handle) = has_auth_task {
        has_auth_handle.await.map_err(anyhow::Error::from)??
    } else {
        // Either logging was disabled or there were no restricted paths
        // Access logging is a pre-requisite for enforcement!
        true
    };

    if !has_auth {
        return Err(RestrictedPathsError::AuthorizationError(
            RestrictedPathAccessType::Path(path),
        ));
    }

    Ok(())
}

/// Helper function to spawn an async task that logs access to restricted paths by manifest ID.
///
/// This function checks if restricted paths access logging is enabled via justknobs,
/// and if so, spawns an async task to log the access. The logging is done asynchronously
/// to avoid blocking the request.
///
/// Only spawns a task when scuba logging is actually enabled (not a discard builder).
/// This avoids unnecessary task spawning overhead when logging is disabled.
///
/// # Arguments
/// * `ctx` - The core context for the operation
/// * `restricted_paths` - Arc to the RestrictedPaths configuration
/// * `manifest_id` - The manifest ID being accessed
/// * `manifest_type` - The type of manifest (e.g., Fsnode, HgManifest)
/// * `switch_value` - The justknobs switch value to use for feature gating
///
/// # Returns
/// Ok(()) if the justknobs check succeeds, Err otherwise
fn spawn_log_restricted_manifest_access(
    ctx: &CoreContext,
    restricted_paths: Arc<RestrictedPaths>,
    manifest_id: ManifestId,
    manifest_type: ManifestType,
    switch_value: &str,
) -> Result<Option<task::JoinHandle<Result<bool>>>> {
    // Early return if logging is disabled - avoid all overhead
    if !justknobs::eval(
        "scm/mononoke:enabled_restricted_paths_access_logging",
        None,
        Some(switch_value),
    )? {
        return Ok(None);
    }

    // Early return if config is empty - no restricted paths to check
    if restricted_paths.config().is_empty() {
        return Ok(None);
    }

    // Only spawn task if we're actually going to log something
    let ctx_clone = ctx.clone();

    // Log asynchronously to avoid blocking the request
    let spawned_task = mononoke::spawn_task(async move {
        restricted_paths
            .log_access_by_manifest_if_restricted(&ctx_clone, manifest_id, manifest_type)
            .await
    });

    // But return the task handle so callers can wait on the access check result
    // if needed.
    Ok(Some(spawned_task))
}

/// Spawn enforcement check for restricted manifest access.
///
/// This function:
/// 1. Calls `spawn_log_restricted_manifest_access` for logging (fire-and-forget)
/// 2. Checks if enforcement JK is enabled
/// 3. Checks if any of the client identities match the condition enforcement ACLs
/// 4. If match AND user lacks authorization, returns `RestrictedPathsError::AuthorizationError`
///
/// # Returns
/// * `Ok(())` if access is allowed or enforcement is disabled
/// * `Err(RestrictedPathsError::AuthorizationError)` if access is denied
pub async fn spawn_enforce_restricted_manifest_access<'a>(
    ctx: &'a CoreContext,
    restricted_paths: Arc<RestrictedPaths>,
    manifest_id: ManifestId,
    manifest_type: ManifestType,
    switch_value: &'a str,
) -> Result<(), RestrictedPathsError<'a>> {
    // Always log first, but get the task handle so we can get the access check
    // result if needed.
    let has_auth_task = spawn_log_restricted_manifest_access(
        ctx,
        restricted_paths.clone(),
        manifest_id.clone(),
        manifest_type.clone(),
        switch_value,
    )?;

    // Check if enforcement JK is enabled
    let enforcement_enabled = justknobs::eval(
        "scm/mononoke:enable_server_side_path_acls",
        None,
        Some(switch_value),
    )?;

    // Early return if enforcement is disabled or there are no conditional
    // enforcement ACLs configured
    if !enforcement_enabled
        || restricted_paths
            .config()
            .conditional_enforcement_acls
            .is_empty()
    {
        return Ok(());
    }

    let should_enforce_restrictions = restricted_paths
        .should_enforce_restriction(ctx)
        .await
        .context("Checking if conditional enforcement ACLs match")?;

    if !should_enforce_restrictions {
        return Ok(());
    }

    // Conditional enforcement matched - check authorization
    let has_auth = if let Some(has_auth_handle) = has_auth_task {
        has_auth_handle.await.map_err(anyhow::Error::from)??
    } else {
        // Either logging was disabled or there were no restricted paths
        // Access logging is a pre-requisite for enforcement!
        true
    };

    if !has_auth {
        return Err(RestrictedPathsError::AuthorizationError(
            RestrictedPathAccessType::Manifest(manifest_id),
        ));
    }

    Ok(())
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
    use repo_derived_data::RepoDerivedDataArc;
    use sql_construct::SqlConstruct;

    use super::*;
    use crate::SqlRestrictedPathsManifestIdStoreBuilder;

    #[facet::container]
    struct MinimalTestRepo(
        repo_derived_data::RepoDerivedData,
        restricted_paths_common::RestrictedPathsConfigBased,
    );

    /// Build a config-based `RestrictedPaths` for tests.
    async fn build_test_restricted_paths(
        fb: FacebookInit,
        config: metaconfig_types::RestrictedPathsConfig,
    ) -> Result<RestrictedPaths> {
        let acl_provider = DummyAclProvider::new(fb)?;
        let test_repo: MinimalTestRepo = test_repo_factory::build_empty(fb).await?;
        let repo_derived_data = test_repo.repo_derived_data_arc();
        let repo_id = RepositoryId::new(0);
        let manifest_id_store = Arc::new(
            SqlRestrictedPathsManifestIdStoreBuilder::with_sqlite_in_memory()?
                .with_repo_id(repo_id),
        );
        let scuba = MononokeScubaSampleBuilder::with_discard();
        let config_based = Arc::new(RestrictedPathsConfigBased::new(
            config,
            manifest_id_store,
            None,
        ));

        RestrictedPaths::new(
            config_based,
            acl_provider,
            scuba,
            false, // use_acl_manifest
            repo_derived_data,
        )
    }

    #[mononoke::fbinit_test]
    async fn test_empty_config(fb: FacebookInit) -> Result<()> {
        let repo_restricted_paths =
            build_test_restricted_paths(fb, RestrictedPathsConfig::default()).await?;

        assert!(!repo_restricted_paths.has_restricted_paths());

        let ctx = CoreContext::test_mock(fb);
        let cs_id = ChangesetId::new(mononoke_types::hash::Blake2::from_byte_array([0u8; 32]));
        let test_path = NonRootMPath::new("test/path")?;
        assert!(
            repo_restricted_paths
                .get_exact_path_restriction(&ctx, Some(cs_id), &[test_path])
                .await?
                .is_empty()
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_with_config(fb: FacebookInit) -> Result<()> {
        let mut path_acls = HashMap::new();
        path_acls.insert(
            NonRootMPath::new("restricted/dir")?,
            MononokeIdentity::from_str("SERVICE_IDENTITY:restricted_acl")?,
        );
        path_acls.insert(
            NonRootMPath::new("other/restricted")?,
            MononokeIdentity::from_str("SERVICE_IDENTITY:other_acl")?,
        );

        let config = RestrictedPathsConfig {
            path_acls,
            use_manifest_id_cache: true,
            cache_update_interval_ms: 100,
            soft_path_acls: Vec::new(),
            conditional_enforcement_acls: Vec::new(),
            tooling_allowlist_group: None,
            acl_file_name: RestrictedPathsConfig::default().acl_file_name,
        };

        let repo_restricted_paths = build_test_restricted_paths(fb, config).await?;

        assert!(repo_restricted_paths.has_restricted_paths());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_path_matching(fb: FacebookInit) -> Result<()> {
        let mut path_acls = HashMap::new();
        let restricted_acl = MononokeIdentity::from_str("SERVICE_IDENTITY:restricted_acl")?;
        path_acls.insert(NonRootMPath::new("restricted/dir")?, restricted_acl.clone());

        let config = RestrictedPathsConfig {
            path_acls,
            use_manifest_id_cache: true,
            cache_update_interval_ms: 100,
            soft_path_acls: Vec::new(),
            conditional_enforcement_acls: Vec::new(),
            tooling_allowlist_group: None,
            acl_file_name: RestrictedPathsConfig::default().acl_file_name,
        };

        let repo_restricted_paths = build_test_restricted_paths(fb, config).await?;

        let ctx = CoreContext::test_mock(fb);
        let cs_id = ChangesetId::new(mononoke_types::hash::Blake2::from_byte_array([0u8; 32]));

        // Test exact match
        let exact_path = NonRootMPath::new("restricted/dir")?;
        let results = repo_restricted_paths
            .get_exact_path_restriction(&ctx, Some(cs_id), &[exact_path])
            .await?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].repo_region_acl, restricted_acl.to_string());

        // Test subdirectory — should NOT match (exact only)
        let sub_path = NonRootMPath::new("restricted/dir/subdir/file.txt")?;
        assert!(
            repo_restricted_paths
                .get_exact_path_restriction(&ctx, Some(cs_id), &[sub_path])
                .await?
                .is_empty()
        );

        // Test non-matching path
        let other_path = NonRootMPath::new("other/dir/file.txt")?;
        assert!(
            repo_restricted_paths
                .get_exact_path_restriction(&ctx, Some(cs_id), &[other_path])
                .await?
                .is_empty()
        );

        // Test path that shares parent directory — should NOT match
        let partial_path = NonRootMPath::new("restricted/different")?;
        assert!(
            repo_restricted_paths
                .get_exact_path_restriction(&ctx, Some(cs_id), &[partial_path])
                .await?
                .is_empty()
        );

        // Test partial match, differing by one character — should NOT match
        let partial_path = NonRootMPath::new("restricted/di")?;
        assert!(
            repo_restricted_paths
                .get_exact_path_restriction(&ctx, Some(cs_id), &[partial_path])
                .await?
                .is_empty()
        );
        Ok(())
    }
}

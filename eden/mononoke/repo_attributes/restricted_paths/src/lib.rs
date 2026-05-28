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
pub(crate) mod restriction_check;
pub(crate) mod restriction_info;

#[cfg(test)]
mod test_utils;

use std::sync::Arc;

use anyhow::Result;
use context::CoreContext;
use ephemeral_blobstore::Bubble;
use metaconfig_types::AclManifestMode;
use metaconfig_types::RestrictedPathsConfig;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
use permission_checker::AclProvider;
use repo_derived_data::ArcRepoDerivedData;
pub use restricted_paths_common::*;
use scuba_ext::MononokeScubaSampleBuilder;
use thiserror::Error;

pub use crate::access_log::ACCESS_LOG_SCUBA_TABLE;
use crate::access_log::log_access_to_restricted_path;
pub use crate::restriction_check::ManifestRestrictionCheckResult;
pub use crate::restriction_check::PathRestrictionCheckResult;
pub use crate::restriction_check::PermissionRequestGroup;
use crate::restriction_check::PreFilterResult;
pub use crate::restriction_check::RestrictionCheckResult;
use crate::restriction_check::SharedFetchHandle;
use crate::restriction_check::SourceRestrictionCheck;
pub use crate::restriction_check::check_path_restriction_infos;
pub use crate::restriction_info::ManifestRestrictionInfo;
pub use crate::restriction_info::PathRestrictionInfo;

#[derive(Clone, Debug)]
pub enum RestrictedPathAccess {
    Manifest(ManifestId),
    Path(MPath),
}

#[derive(Clone, Debug, Error)]
#[error("Access denied: unauthorized access to restricted path: {access}")]
pub struct RestrictedPathsAuthorizationError {
    access: RestrictedPathAccess,
    permission_request_group: PermissionRequestGroup,
}

impl RestrictedPathsAuthorizationError {
    pub fn new(
        access: RestrictedPathAccess,
        permission_request_group: PermissionRequestGroup,
    ) -> Self {
        Self {
            access,
            permission_request_group,
        }
    }

    pub fn permission_request_group(&self) -> &PermissionRequestGroup {
        &self.permission_request_group
    }

    pub fn is_manifest_access(&self) -> bool {
        matches!(&self.access, RestrictedPathAccess::Manifest(_))
    }
}

/// Error type for restricted paths enforcement.
#[derive(Debug, Error)]
pub enum RestrictedPathsError {
    #[error(transparent)]
    AuthorizationError(RestrictedPathsAuthorizationError),
    #[error("Internal error: {0}")]
    InternalError(#[from] anyhow::Error),
}

impl std::fmt::Display for RestrictedPathAccess {
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
    /// Repo derived data for deriving ACL manifests.
    repo_derived_data: ArcRepoDerivedData,
}

impl RestrictedPaths {
    pub fn new(
        config_based: Arc<RestrictedPathsConfigBased>,
        acl_provider: Arc<dyn AclProvider>,
        scuba: MononokeScubaSampleBuilder,
        repo_derived_data: ArcRepoDerivedData,
    ) -> Result<Self> {
        if !config_based.config().acl_manifest_mode.is_disabled() {
            anyhow::ensure!(
                repo_derived_data
                    .config()
                    .is_enabled(DerivableType::AclManifests),
                "acl_manifest_mode is enabled but AclManifest derivation is not enabled for this repo. \
                 Enable AclManifests in the repo's derived data config."
            );
        }
        Ok(Self {
            config_based,
            acl_provider,
            scuba,
            repo_derived_data,
        })
    }

    /// Return a copy of `self` whose `repo_derived_data` is wrapped for the
    /// given bubble, so AclManifest derivation runs against the bubble's
    /// derived-data manager rather than the persistent one.
    pub fn for_bubble(&self, bubble: Bubble) -> Self {
        Self {
            config_based: self.config_based.clone(),
            acl_provider: self.acl_provider.clone(),
            scuba: self.scuba.clone(),
            repo_derived_data: Arc::new(self.repo_derived_data.for_bubble(bubble)),
        }
    }

    // TODO(T248660053): make pub(crate) once hooks use dedicated primitives
    // instead of accessing path_acls directly. Blocked on adding a primitive
    // for "is path related to any restriction root" (used by block_restricted_copy
    // and block_restricted_subtree_copy hooks).
    pub fn config(&self) -> &RestrictedPathsConfig {
        self.config_based.config()
    }

    /// Returns whether this repository may have restricted paths.
    /// When AclManifest mode is enabled, restrictions can be discovered
    /// dynamically from `.slacl` files in the repo, so callers cannot treat a
    /// false config-only lookup as proof that no restricted paths exist.
    pub fn may_have_restricted_paths(&self) -> bool {
        // TODO(T248658346): account for manifest-id store entries that were
        // recorded for paths that have since been removed from the current
        // config. Historical manifests for those paths should still be treated
        // as restricted.
        !self.config().acl_manifest_mode.is_disabled() || self.config_based.has_restricted_paths()
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

    // -----------------------------------------------------------------------
    // Public restriction lookup methods
    // -----------------------------------------------------------------------

    /// Get restriction info for paths that are themselves restriction roots.
    /// Does NOT consider parent directories.
    pub async fn get_path_restriction_root_info(
        &self,
        ctx: &CoreContext,
        cs_id: Option<ChangesetId>,
        paths: &[NonRootMPath],
    ) -> Result<Vec<PathRestrictionInfo>> {
        restriction_info::get_path_restriction_root_info(self, ctx, cs_id, paths).await
    }

    /// Get restriction info for one or more paths, considering ancestor restrictions.
    /// For each path, collects restrictions at every ancestor directory.
    pub async fn get_path_restriction_info(
        &self,
        ctx: &CoreContext,
        cs_id: Option<ChangesetId>,
        paths: &[NonRootMPath],
    ) -> Result<Vec<PathRestrictionInfo>> {
        restriction_info::get_path_restriction_info(self, ctx, cs_id, paths).await
    }

    /// Get restriction and authorization checks for paths that are themselves restriction roots.
    /// Does NOT consider parent directories.
    pub async fn get_path_restriction_root_check(
        &self,
        ctx: &CoreContext,
        cs_id: Option<ChangesetId>,
        paths: &[NonRootMPath],
    ) -> Result<Vec<PathRestrictionCheckResult>> {
        restriction_check::get_path_restriction_root_check(self, ctx, cs_id, paths).await
    }

    /// Get restriction and authorization checks for one or more paths,
    /// considering ancestor restrictions.
    pub async fn get_path_restriction_check(
        &self,
        ctx: &CoreContext,
        cs_id: Option<ChangesetId>,
        paths: &[NonRootMPath],
    ) -> Result<Vec<PathRestrictionCheckResult>> {
        restriction_check::get_path_restriction_check(self, ctx, cs_id, paths).await
    }

    /// Get manifest restriction and authorization checks from the config-backed source.
    pub async fn get_manifest_restriction_check(
        &self,
        ctx: &CoreContext,
        manifest_id: &ManifestId,
        manifest_type: &ManifestType,
    ) -> Result<Vec<ManifestRestrictionCheckResult>> {
        restriction_check::get_manifest_restriction_check_for_current_behavior(
            self,
            ctx,
            manifest_id,
            manifest_type,
        )
        .await
    }

    /// Get manifest restriction info without performing authorization checks.
    pub async fn get_manifest_restriction_info(
        &self,
        ctx: &CoreContext,
        manifest_id: &ManifestId,
        manifest_type: &ManifestType,
    ) -> Result<Vec<ManifestRestrictionInfo>> {
        restriction_info::get_manifest_restriction_info(self, ctx, manifest_id, manifest_type).await
    }

    /// Check if a path is itself a restriction root (exact match).
    /// Returns false for paths that are merely under a restriction root.
    pub async fn is_restriction_root(
        &self,
        ctx: &CoreContext,
        cs_id: Option<ChangesetId>,
        path: &NonRootMPath,
    ) -> Result<bool> {
        restriction_info::is_restriction_root(self, ctx, cs_id, path).await
    }

    /// Check if a path is restricted, considering ancestor directories.
    /// Returns true if the path itself or any of its ancestors is a restriction root.
    pub async fn is_restricted_path(
        &self,
        ctx: &CoreContext,
        cs_id: Option<ChangesetId>,
        path: &NonRootMPath,
    ) -> Result<bool> {
        restriction_info::is_restricted_path(self, ctx, cs_id, path).await
    }

    /// Returns whether this HgAugmented manifest is a restriction root
    /// according to the sources enabled by `AclManifestMode`.
    ///
    /// `preloaded_is_restricted` is the AclManifest-backed restriction bit that
    /// the caller already loaded with the HgAugmented manifest. This primitive
    /// combines that value with the config-backed manifest-id store when the
    /// selected mode requires it, without reloading AclManifest data.
    pub async fn is_restricted_manifest(
        &self,
        ctx: &CoreContext,
        manifest_id: &ManifestId,
        manifest_type: &ManifestType,
        preloaded_is_restricted: bool,
    ) -> Result<bool> {
        restriction_info::is_restricted_manifest(
            self,
            ctx,
            manifest_id,
            manifest_type,
            preloaded_is_restricted,
        )
        .await
    }

    /// Find all restriction roots that are descendants of any of the given root paths.
    /// Results are deduplicated by restriction_root.
    pub async fn find_restricted_descendants(
        &self,
        ctx: &CoreContext,
        cs_id: Option<ChangesetId>,
        roots: Vec<MPath>,
    ) -> Result<Vec<PathRestrictionInfo>> {
        restriction_info::find_restricted_descendants(self, ctx, cs_id, roots).await
    }

    // -----------------------------------------------------------------------
    // Public access logging methods
    // -----------------------------------------------------------------------

    /// Check if a manifest id belongs to a restricted path and log access to it.
    ///
    /// Returns a `RestrictionCheckResult` with authorization status and
    /// restriction root info.
    pub async fn log_access_by_manifest_if_restricted(
        &self,
        ctx: &CoreContext,
        manifest_id: ManifestId,
        manifest_type: ManifestType,
        cs_id: Option<ChangesetId>,
    ) -> Result<RestrictionCheckResult> {
        let acl_manifest_mode = self.config().acl_manifest_mode;
        if matches!(
            acl_manifest_mode,
            AclManifestMode::Shadow | AclManifestMode::Both
        ) {
            return access_log::log_source_comparison_access_by_manifest_if_restricted(
                self,
                ctx,
                manifest_id,
                manifest_type,
                acl_manifest_mode,
            )
            .await;
        }

        // No need to query the DB if the config is empty, i.e. the repo doesn't
        // have any restricted paths.
        let _ = cs_id; // Config-backed manifest logging does not use changeset ids.

        if self.config().is_empty() {
            return Ok(RestrictionCheckResult {
                has_authorization: true,
                restriction_roots: vec![],
            });
        }

        let paths = restriction_info::get_manifest_restricted_paths_from_config(
            self,
            ctx,
            &manifest_id,
            &manifest_type,
        )
        .await?;

        if paths.is_empty() {
            return Ok(RestrictionCheckResult {
                has_authorization: true,
                restriction_roots: vec![],
            });
        }

        // Use config-based lookup directly — this method works with manifest IDs
        // from the restricted paths store, not with changesets, so we always use
        // the config to determine which paths are restricted.
        // TODO(T248660053): support manifest-based access using AclManifests.
        let acls = restriction_info::get_config_acls_for_paths(self, &paths);

        log_access_to_restricted_path(
            ctx,
            self.config_based.manifest_id_store().repo_id(),
            paths,
            acls,
            crate::access_log::RestrictedPathAccessData::Manifest(manifest_id, manifest_type),
            self.config().acl_manifest_mode,
            self.acl_provider.clone(),
            self.config().tooling_allowlist_group.as_deref(),
            self.config().rollout_allowlist_group.as_deref(),
            self.scuba.clone(),
            vec!["manifest_db".to_string()],
        )
        .await
    }

    /// Log access to a restricted path, when it's accessed by the full path,
    /// instead of a manifest id.
    ///
    /// Returns a `RestrictionCheckResult` with authorization status and
    /// restriction root info.
    pub async fn log_access_by_path_if_restricted(
        &self,
        ctx: &CoreContext,
        path: NonRootMPath,
        cs_id: Option<ChangesetId>,
    ) -> Result<RestrictionCheckResult> {
        let acl_manifest_mode = self.config().acl_manifest_mode;
        if matches!(
            acl_manifest_mode,
            AclManifestMode::Shadow | AclManifestMode::Both
        ) {
            return access_log::log_source_comparison_access_by_path_if_restricted(
                self,
                ctx,
                path,
                cs_id,
                acl_manifest_mode,
            )
            .await;
        }

        // Return early if the repo doesn't have any restricted paths.
        let _ = cs_id; // Config-backed path logging does not use changeset ids.
        if self.config().is_empty() {
            return Ok(RestrictionCheckResult {
                has_authorization: true,
                restriction_roots: vec![],
            });
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
            return Ok(RestrictionCheckResult {
                has_authorization: true,
                restriction_roots: vec![],
            });
        }

        log_access_to_restricted_path(
            ctx,
            self.config_based.manifest_id_store().repo_id(),
            restricted_path_roots,
            matched_acls,
            crate::access_log::RestrictedPathAccessData::FullPath { full_path: path },
            self.config().acl_manifest_mode,
            self.acl_provider.clone(),
            self.config().tooling_allowlist_group.as_deref(),
            self.config().rollout_allowlist_group.as_deref(),
            self.scuba.clone(),
            vec!["manifest_db".to_string()],
        )
        .await
    }
}

/// Spawn enforcement check for restricted path access.
///
/// This function:
/// 1. Spawns any source fetches needed by logging or enforcement
/// 2. Spawns logging as a fire-and-forget task when logging is enabled
/// 3. Checks whether enforcement is enabled for a matching condition set
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
    cs_id: Option<ChangesetId>,
) -> Result<(), RestrictedPathsError> {
    let non_root_mpath = match NonRootMPath::try_from(path.clone()) {
        Ok(path) => path,
        Err(_) => return Ok(()),
    };
    let config = restricted_paths.config();
    let effective_mode = effective_acl_manifest_mode(config.acl_manifest_mode, true);
    let config_path_may_restrict = config
        .path_acls
        .keys()
        .any(|prefix| prefix.is_prefix_of(&non_root_mpath));
    let access_data = access_log::RestrictedPathAccessData::FullPath {
        full_path: non_root_mpath.clone(),
    };

    spawn_enforce_restricted_access(
        ctx,
        restricted_paths.clone(),
        switch_value,
        effective_mode,
        config_path_may_restrict,
        cs_id.is_some(),
        "path",
        access_data,
        move |fetches| SourceHandles {
            config: fetches.fetch_config().then(|| {
                if !config_path_may_restrict {
                    return SharedFetchHandle::from_result(Ok(
                        Vec::<PathRestrictionCheckResult>::new(),
                    ));
                }
                let ctx = ctx.clone();
                let restricted_paths = restricted_paths.clone();
                let path = non_root_mpath.clone();
                SharedFetchHandle::from_future(async move {
                    restriction_check::check_path_restriction_from_source(
                        &ctx,
                        &restricted_paths,
                        path,
                        restriction_check::PathRestrictionSource::Config,
                    )
                    .await
                })
            }),
            acl_manifest: fetches
                .fetch_acl_manifest()
                .then(|| {
                    cs_id.map(|cs_id| {
                        restriction_check::spawn_path_restriction_check(
                            ctx,
                            restricted_paths.clone(),
                            non_root_mpath.clone(),
                            restriction_check::PathRestrictionSource::AclManifest(cs_id),
                        )
                    })
                })
                .flatten(),
        },
        move |permission_request_group| {
            RestrictedPathsError::AuthorizationError(RestrictedPathsAuthorizationError::new(
                RestrictedPathAccess::Path((*path).clone()),
                permission_request_group,
            ))
        },
    )
    .await
}

/// Spawn enforcement check for restricted manifest access.
///
/// This function:
/// 1. Spawns any source fetches needed by logging or enforcement
/// 2. Spawns logging as a fire-and-forget task when logging is enabled
/// 3. Checks whether enforcement is enabled for a matching condition set
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
    _cs_id: Option<ChangesetId>,
) -> Result<(), RestrictedPathsError> {
    let config = restricted_paths.config();
    let acl_manifest_available = manifest_type == ManifestType::HgAugmented;
    let effective_mode =
        effective_acl_manifest_mode(config.acl_manifest_mode, acl_manifest_available);
    // TODO(T248658346): decouple config from manifest_id store. We should continue enforcing if the config doesn't have the path, but the manifest_id store has entries for the manifest.
    let config_manifest_may_restrict = !config.is_empty();
    let access_data =
        access_log::RestrictedPathAccessData::Manifest(manifest_id.clone(), manifest_type.clone());
    let manifest_id_for_fetches = manifest_id.clone();
    let manifest_type_for_fetches = manifest_type.clone();

    spawn_enforce_restricted_access(
        ctx,
        restricted_paths.clone(),
        switch_value,
        effective_mode,
        config_manifest_may_restrict,
        acl_manifest_available,
        "manifest",
        access_data,
        move |fetches| SourceHandles {
            config: fetches.fetch_config().then(|| {
                if !config_manifest_may_restrict {
                    return SharedFetchHandle::from_result(Ok(
                        Vec::<ManifestRestrictionCheckResult>::new(),
                    ));
                }
                let ctx = ctx.clone();
                let restricted_paths = restricted_paths.clone();
                let manifest_id = manifest_id_for_fetches.clone();
                let manifest_type = manifest_type_for_fetches.clone();
                SharedFetchHandle::from_future(async move {
                    restriction_check::check_manifest_restriction_from_source(
                        &ctx,
                        &restricted_paths,
                        manifest_id,
                        manifest_type,
                        restriction_check::ManifestRestrictionSource::Config,
                    )
                    .await
                })
            }),
            acl_manifest: fetches
                .fetch_acl_manifest()
                .then(|| {
                    acl_manifest_available.then(|| {
                        restriction_check::spawn_manifest_restriction_check(
                            ctx,
                            restricted_paths.clone(),
                            manifest_id_for_fetches.clone(),
                            manifest_type_for_fetches.clone(),
                            restriction_check::ManifestRestrictionSource::AclManifest,
                        )
                    })
                })
                .flatten(),
        },
        move |permission_request_group| {
            RestrictedPathsError::AuthorizationError(RestrictedPathsAuthorizationError::new(
                RestrictedPathAccess::Manifest(manifest_id),
                permission_request_group,
            ))
        },
    )
    .await
}

const ACCESS_LOGGING_JK: &str = "scm/mononoke:enabled_restricted_paths_access_logging";

struct SourceHandles<T: SourceRestrictionCheck> {
    config: Option<SharedFetchHandle<T>>,
    acl_manifest: Option<SharedFetchHandle<T>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceFetchOptions {
    logging_enabled: bool,
    enforcement_enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceFetches {
    logging_config: bool,
    logging_acl_manifest: bool,
    enforcement_config: bool,
    enforcement_acl_manifest: bool,
}

impl SourceFetches {
    fn fetch_config(self) -> bool {
        self.logging_config || self.enforcement_config
    }

    fn fetch_acl_manifest(self) -> bool {
        self.logging_acl_manifest || self.enforcement_acl_manifest
    }
}

/// Shared path/manifest enforcement orchestration.
///
/// The public entrypoints decide how to build source handles for their access
/// type. This helper owns the common flow: evaluate cheap request-local
/// enforcement filters first, choose the sources needed by logging and
/// enforcement, spawn fire-and-forget logging from cloned shared handles, and
/// finally await only the authoritative handles needed to deny the request.
async fn spawn_enforce_restricted_access<T>(
    ctx: &CoreContext,
    restricted_paths: Arc<RestrictedPaths>,
    switch_value: &str,
    effective_mode: AclManifestMode,
    config_source_may_restrict: bool,
    acl_manifest_available: bool,
    access_type: &'static str,
    access_data: access_log::RestrictedPathAccessData,
    build_handles: impl FnOnce(SourceFetches) -> SourceHandles<T>,
    authorization_error: impl FnOnce(PermissionRequestGroup) -> RestrictedPathsError,
) -> Result<(), RestrictedPathsError>
where
    T: SourceRestrictionCheck + Send + Sync + 'static,
{
    let config = restricted_paths.config();
    let enforcement_enabled =
        config.enforcement_enabled && !config.enforcement_condition_sets.is_empty();
    let source_options = fetch_options_for_access(switch_value, enforcement_enabled)?;
    if !source_options.logging_enabled && !source_options.enforcement_enabled {
        return Ok(());
    }

    let pre_filter_result = if enforcement_enabled {
        restriction_check::pre_filter_condition_sets(ctx, &config.enforcement_condition_sets)
    } else {
        PreFilterResult::NoMatch
    };
    let fetches = source_fetches_for_access(
        source_options,
        &pre_filter_result,
        effective_mode,
        config_source_may_restrict,
        acl_manifest_available,
    );
    let handles = build_handles(fetches);

    if !source_options.enforcement_enabled {
        access_log::spawn_log_source_results_with_enforcement(
            ctx,
            restricted_paths.clone(),
            access_data,
            effective_mode,
            None,
            fetches
                .logging_config
                .then(|| handles.config.clone())
                .flatten(),
            fetches
                .logging_acl_manifest
                .then(|| handles.acl_manifest.clone())
                .flatten(),
        );
        return Ok(());
    }

    let enforcement_outcome = enforce_with_source_handles(
        fetches.enforcement_config,
        fetches.enforcement_acl_manifest,
        &handles,
        pre_filter_result,
        missing_authoritative_source_error(
            access_type,
            effective_mode,
            fetches.enforcement_config,
            fetches.enforcement_acl_manifest,
        ),
    )
    .await;

    access_log::spawn_log_source_results_with_enforcement(
        ctx,
        restricted_paths.clone(),
        access_data,
        effective_mode,
        enforcement_outcome
            .as_ref()
            .ok()
            .map(|outcome| outcome.access_enforcement_enabled),
        fetches
            .logging_config
            .then(|| handles.config.clone())
            .flatten(),
        fetches
            .logging_acl_manifest
            .then(|| handles.acl_manifest.clone())
            .flatten(),
    );

    let enforcement_outcome = enforcement_outcome?;

    if let Some(permission_request_group) = enforcement_outcome.denial_permission_request_group {
        Err(authorization_error(permission_request_group))
    } else {
        Ok(())
    }
}

/// Read per-request feature switches that affect source fetching.
///
/// `enforcement_enabled` is passed in after config-level enforcement checks so
/// source selection can treat disabled enforcement like a request that did not
/// match any enforcement condition set.
fn fetch_options_for_access(
    switch_value: &str,
    enforcement_enabled: bool,
) -> Result<SourceFetchOptions> {
    Ok(SourceFetchOptions {
        logging_enabled: justknobs::eval(ACCESS_LOGGING_JK, None, Some(switch_value)),
        enforcement_enabled,
    })
}

/// Decide which restriction sources must be fetched for this access.
///
/// Logging and enforcement choose sources independently: comparison logging can
/// need both config and AclManifest even when enforcement only needs the
/// authoritative source. Centralizing the source table keeps path and manifest
/// callsites from duplicating those mode rules.
fn source_fetches_for_access(
    source_options: SourceFetchOptions,
    pre_filter_result: &PreFilterResult<'_>,
    effective_mode: AclManifestMode,
    config_source_may_restrict: bool,
    acl_manifest_available: bool,
) -> SourceFetches {
    let enforcement_fetch_enabled = !matches!(pre_filter_result, PreFilterResult::NoMatch)
        && source_options.enforcement_enabled;
    let enforcement_config =
        enforcement_fetch_enabled && fetch_config_for_enforcement(effective_mode);
    let enforcement_acl_manifest =
        enforcement_fetch_enabled && fetch_acl_manifest_for_enforcement(effective_mode);
    let logging_acl_manifest = source_options.logging_enabled
        && fetch_acl_manifest_for_logging(effective_mode)
        && acl_manifest_available;
    let logging_config = source_options.logging_enabled
        && effective_mode != AclManifestMode::Authoritative
        && (config_source_may_restrict
            || (compares_sources_for_logging(effective_mode) && logging_acl_manifest));

    SourceFetches {
        logging_config,
        logging_acl_manifest,
        enforcement_config,
        enforcement_acl_manifest,
    }
}

fn missing_authoritative_source_error(
    access_type: &str,
    acl_manifest_mode: metaconfig_types::AclManifestMode,
    needs_config: bool,
    needs_acl_manifest: bool,
) -> anyhow::Error {
    anyhow::anyhow!(
        "authoritative {access_type} source for acl_manifest_mode={:?} unavailable (needs_config={}, needs_acl_manifest={})",
        acl_manifest_mode,
        needs_config,
        needs_acl_manifest,
    )
}

struct SelectedSourceHandles<'a, T: SourceRestrictionCheck> {
    handles: Vec<&'a SharedFetchHandle<T>>,
    missing_source: bool,
}

impl<T: SourceRestrictionCheck> SourceHandles<T> {
    fn selected_for(
        &self,
        fetch_config: bool,
        fetch_acl_manifest: bool,
    ) -> SelectedSourceHandles<'_, T> {
        let mut handles = Vec::new();
        let mut missing_source = false;

        if fetch_config {
            if let Some(handle) = self.config.as_ref() {
                handles.push(handle);
            } else {
                missing_source = true;
            }
        }
        if fetch_acl_manifest {
            if let Some(handle) = self.acl_manifest.as_ref() {
                handles.push(handle);
            } else {
                missing_source = true;
            }
        }

        SelectedSourceHandles {
            handles,
            missing_source,
        }
    }
}

/// Normalize the repo-configured rollout mode to the mode this access can use.
///
/// `AclManifestMode` is configured at the repo level, but AclManifest is not
/// always an available source for an individual access: path checks need a
/// changeset id, and manifest checks need a manifest type backed by
/// AclManifest data. This helper is the single boundary where callsites combine
/// configured mode with source availability before source selection.
///
/// In this diff only Shadow comparison can use AclManifest, so non-Shadow modes
/// collapse to Disabled. Follow-up diffs extend the match arms for Both and
/// Authoritative without forcing each path/manifest callsite to duplicate the
/// availability fallback logic.
fn effective_acl_manifest_mode(
    acl_manifest_mode: AclManifestMode,
    acl_manifest_supported: bool,
) -> AclManifestMode {
    match acl_manifest_mode {
        AclManifestMode::Shadow => AclManifestMode::Shadow,
        // TODO(T248658346): ensure access through all other manifest types is
        // deprecated
        AclManifestMode::Both if acl_manifest_supported => AclManifestMode::Both,
        AclManifestMode::Authoritative if acl_manifest_supported => AclManifestMode::Authoritative,
        _ => AclManifestMode::Disabled,
    }
}

/// Whether config should be fetched as an authoritative enforcement source.
///
/// These mode predicates intentionally keep source selection as a small table:
/// follow-up diffs can change one predicate when enabling a rollout mode
/// without rewriting the shared fetch orchestration.
fn fetch_config_for_enforcement(effective_mode: AclManifestMode) -> bool {
    effective_mode != AclManifestMode::Authoritative
}

/// Whether AclManifest should be fetched as an authoritative enforcement source.
fn fetch_acl_manifest_for_enforcement(effective_mode: AclManifestMode) -> bool {
    matches!(
        effective_mode,
        AclManifestMode::Both | AclManifestMode::Authoritative
    )
}

/// Whether AclManifest should be fetched for access-log telemetry.
fn fetch_acl_manifest_for_logging(effective_mode: AclManifestMode) -> bool {
    matches!(
        effective_mode,
        AclManifestMode::Shadow | AclManifestMode::Both | AclManifestMode::Authoritative
    )
}

/// Whether logging needs both sources to produce source-comparison fields.
fn compares_sources_for_logging(effective_mode: AclManifestMode) -> bool {
    matches!(
        effective_mode,
        AclManifestMode::Shadow | AclManifestMode::Both
    )
}

async fn enforce_with_source_handles<'a, T>(
    fetch_config: bool,
    fetch_acl_manifest: bool,
    handles: &SourceHandles<T>,
    pre_filter_result: PreFilterResult<'a>,
    missing_source_error: anyhow::Error,
) -> Result<restriction_check::AccessEnforcementOutcome>
where
    T: SourceRestrictionCheck + Send + Sync + 'static,
{
    let (candidates, pre_filter_variant) = match pre_filter_result {
        PreFilterResult::NoMatch => {
            return Ok(restriction_check::AccessEnforcementOutcome {
                access_enforcement_enabled: false,
                denial_permission_request_group: None,
            });
        }
        PreFilterResult::DefiniteMatch { candidates } => {
            (candidates, restriction_check::PreFilterVariant::Definite)
        }
        PreFilterResult::NeedsFetch { candidates } => {
            (candidates, restriction_check::PreFilterVariant::NeedsFetch)
        }
    };

    let selected_sources = handles.selected_for(fetch_config, fetch_acl_manifest);
    let mut source_outcomes =
        futures::future::join_all(selected_sources.handles.into_iter().map(|handle| {
            restriction_check::source_enforcement_outcome(handle, &candidates, &pre_filter_variant)
        }))
        .await;
    if selected_sources.missing_source {
        source_outcomes.push(Err(missing_source_error));
    }

    restriction_check::authoritative_sources_enforcement_outcome(source_outcomes)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::str::FromStr;

    use anyhow::Result;
    use fbinit::FacebookInit;
    use metaconfig_types::AclManifestMode;
    use mononoke_macros::mononoke;
    use mononoke_types::NonRootMPath;
    use permission_checker::MononokeIdentity;
    use permission_checker::dummy::DummyAclProvider;

    use super::*;
    use crate::test_utils::RestrictedPathsConfigBuilder;
    use crate::test_utils::build_test_restricted_paths_with_dummy_acl_provider as build_test_restricted_paths;
    use crate::test_utils::build_test_restricted_paths_with_options;

    #[mononoke::fbinit_test]
    async fn test_empty_config(fb: FacebookInit) -> Result<()> {
        let repo_restricted_paths =
            build_test_restricted_paths(fb, RestrictedPathsConfig::default()).await?;

        assert!(!repo_restricted_paths.may_have_restricted_paths());

        let ctx = CoreContext::test_mock(fb);
        let cs_id = ChangesetId::new(mononoke_types::hash::Blake2::from_byte_array([0u8; 32]));
        let test_path = NonRootMPath::new("test/path")?;
        assert!(
            repo_restricted_paths
                .get_path_restriction_root_info(&ctx, Some(cs_id), &[test_path])
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
            enforcement_condition_sets: Vec::new(),
            enforcement_enabled: RestrictedPathsConfig::default().enforcement_enabled,
            tooling_allowlist_group: None,
            rollout_allowlist_group: None,
            acl_file_name: RestrictedPathsConfig::default().acl_file_name,
            acl_manifest_mode: RestrictedPathsConfig::default().acl_manifest_mode,
        };

        let repo_restricted_paths = build_test_restricted_paths(fb, config).await?;

        assert!(repo_restricted_paths.may_have_restricted_paths());
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
            enforcement_condition_sets: Vec::new(),
            enforcement_enabled: RestrictedPathsConfig::default().enforcement_enabled,
            tooling_allowlist_group: None,
            rollout_allowlist_group: None,
            acl_file_name: RestrictedPathsConfig::default().acl_file_name,
            acl_manifest_mode: RestrictedPathsConfig::default().acl_manifest_mode,
        };

        let repo_restricted_paths = build_test_restricted_paths(fb, config).await?;

        let ctx = CoreContext::test_mock(fb);
        let cs_id = ChangesetId::new(mononoke_types::hash::Blake2::from_byte_array([0u8; 32]));

        // Test exact match
        let exact_path = NonRootMPath::new("restricted/dir")?;
        let results = repo_restricted_paths
            .get_path_restriction_root_info(&ctx, Some(cs_id), &[exact_path])
            .await?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].repo_region_acl, restricted_acl.to_string());

        // Test subdirectory — should NOT match (exact only)
        let sub_path = NonRootMPath::new("restricted/dir/subdir/file.txt")?;
        assert!(
            repo_restricted_paths
                .get_path_restriction_root_info(&ctx, Some(cs_id), &[sub_path])
                .await?
                .is_empty()
        );

        // Test non-matching path
        let other_path = NonRootMPath::new("other/dir/file.txt")?;
        assert!(
            repo_restricted_paths
                .get_path_restriction_root_info(&ctx, Some(cs_id), &[other_path])
                .await?
                .is_empty()
        );

        // Test path that shares parent directory — should NOT match
        let partial_path = NonRootMPath::new("restricted/different")?;
        assert!(
            repo_restricted_paths
                .get_path_restriction_root_info(&ctx, Some(cs_id), &[partial_path])
                .await?
                .is_empty()
        );

        // Test partial match, differing by one character — should NOT match
        let partial_path = NonRootMPath::new("restricted/di")?;
        assert!(
            repo_restricted_paths
                .get_path_restriction_root_info(&ctx, Some(cs_id), &[partial_path])
                .await?
                .is_empty()
        );
        Ok(())
    }

    // What it tests: default Disabled mode does not require AclManifest derived data.
    // Expected: construction succeeds without AclManifest derived data.
    #[mononoke::fbinit_test]
    async fn test_disabled_mode_does_not_require_acl_manifest_derivation(
        fb: FacebookInit,
    ) -> Result<()> {
        let config = RestrictedPathsConfig {
            acl_manifest_mode: AclManifestMode::Disabled,
            ..Default::default()
        };
        let _restricted_paths =
            build_test_restricted_paths_with_options(fb, config, DummyAclProvider::new(fb)?, false)
                .await?;

        assert!(!_restricted_paths.may_have_restricted_paths());
        Ok(())
    }

    // What it tests: Shadow mode requires AclManifest derived data.
    // Expected: construction fails with a clear configuration error.
    #[mononoke::fbinit_test]
    async fn test_shadow_mode_requires_acl_manifest_derivation(fb: FacebookInit) -> Result<()> {
        let config = RestrictedPathsConfig {
            acl_manifest_mode: AclManifestMode::Shadow,
            ..Default::default()
        };
        let result =
            build_test_restricted_paths_with_options(fb, config, DummyAclProvider::new(fb)?, false)
                .await;

        let err = result.err().ok_or_else(|| {
            anyhow::anyhow!(
                "expected Shadow construction to fail when AclManifest derivation is disabled"
            )
        })?;
        assert_error_chain_contains(&err, "acl_manifest_mode is enabled");
        assert_error_chain_contains(&err, "AclManifest derivation is not enabled");
        Ok(())
    }

    // What it tests: Both mode should require AclManifest derived data because
    // it must fetch restrictions from AclManifest as an authoritative source.
    // Expected: construction fails with a clear configuration error.
    #[mononoke::fbinit_test]
    async fn test_both_mode_requires_acl_manifest_derivation(fb: FacebookInit) -> Result<()> {
        let config = RestrictedPathsConfig {
            acl_manifest_mode: AclManifestMode::Both,
            ..Default::default()
        };
        let result =
            build_test_restricted_paths_with_options(fb, config, DummyAclProvider::new(fb)?, false)
                .await;

        let err = result.err().ok_or_else(|| {
            anyhow::anyhow!(
                "expected Both construction to fail when AclManifest derivation is disabled"
            )
        })?;
        assert_error_chain_contains(&err, "acl_manifest_mode is enabled");
        assert_error_chain_contains(&err, "AclManifest derivation is not enabled");
        Ok(())
    }

    // What it tests: Authoritative mode should require AclManifest derived data
    // because config should no longer be the primary restriction source.
    // Expected: construction fails with a clear configuration error.
    #[mononoke::fbinit_test]
    async fn test_authoritative_mode_requires_acl_manifest_derivation(
        fb: FacebookInit,
    ) -> Result<()> {
        let config = RestrictedPathsConfig {
            acl_manifest_mode: AclManifestMode::Authoritative,
            ..Default::default()
        };
        let result =
            build_test_restricted_paths_with_options(fb, config, DummyAclProvider::new(fb)?, false)
                .await;

        let err = result.err().ok_or_else(|| {
            anyhow::anyhow!(
                "expected Authoritative construction to fail when AclManifest derivation is disabled"
            )
        })?;
        assert_error_chain_contains(&err, "acl_manifest_mode is enabled");
        assert_error_chain_contains(&err, "AclManifest derivation is not enabled");
        Ok(())
    }

    // What it tests: default Disabled mode keeps config-backed lookup behavior.
    // Expected: the config source remains authoritative in Disabled mode.
    #[mononoke::fbinit_test]
    async fn test_disabled_mode_keeps_config_authoritative_lookup(fb: FacebookInit) -> Result<()> {
        let config = RestrictedPathsConfigBuilder::new()
            .with_path_acl_str("restricted/dir", "SERVICE_IDENTITY:restricted_acl")?
            .build();
        let restricted_paths =
            build_test_restricted_paths_with_options(fb, config, DummyAclProvider::new(fb)?, false)
                .await?;
        let ctx = CoreContext::test_mock(fb);
        let cs_id = ChangesetId::new(mononoke_types::hash::Blake2::from_byte_array([0u8; 32]));
        let path = NonRootMPath::new("restricted/dir/file")?;
        let lookup = restricted_paths
            .get_path_restriction_info(&ctx, Some(cs_id), &[path])
            .await?;

        assert_eq!(lookup.len(), 1);
        assert_eq!(lookup[0].repo_region_acl, "SERVICE_IDENTITY:restricted_acl");
        Ok(())
    }

    fn assert_error_chain_contains(err: &anyhow::Error, needle: &str) {
        assert!(
            err.chain().any(|err| err.to_string().contains(needle)),
            "error chain should contain '{needle}', got: {err:?}"
        );
    }
}

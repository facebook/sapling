/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Restriction check helpers that turn restriction lookup results into
//! authorization results.

use std::future::Future;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use context::CoreContext;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::future::BoxFuture;
use futures::future::Shared;
use futures::stream;
use metaconfig_types::EnforcementConditionSet;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use permission_checker::AclProvider;
use permission_checker::MononokeIdentity;
use tokio::task::JoinHandle;

use crate::ManifestId;
use crate::ManifestType;
use crate::RestrictedPaths;
use crate::access_log::has_read_access_to_repo_region_acls;
use crate::access_log::is_part_of_group;
use crate::restriction_info;
use crate::restriction_info::ManifestRestrictionInfo;
use crate::restriction_info::PathRestrictionInfo;

#[cfg(test)]
mod tests;

/// Source to use for path-side restriction checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PathRestrictionSource {
    /// Config-backed path ACLs.
    Config,
    /// AclManifest at a specific changeset.
    AclManifest(ChangesetId),
}

/// Source to use for manifest-side restriction checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ManifestRestrictionSource {
    /// Config-backed manifest-id store.
    Config,
    /// AclManifest pointer attached to the manifest.
    AclManifest,
}

/// Result from restricted path access check — carries both authorization
/// and restriction root info for enforcement condition evaluation.
#[derive(Debug, Clone)]
pub struct RestrictionCheckResult {
    /// Whether the caller has read authorization for the restriction.
    pub has_authorization: bool,
    /// The restriction root paths matched by this access check.
    /// Empty if the path is not restricted.
    pub restriction_roots: Vec<NonRootMPath>,
}

/// Authorization flags produced by evaluating the caller against one source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AuthorizationCheckResult {
    /// Whether the caller has direct read access to every matching path ACL.
    has_acl_access: bool,
    /// Whether the caller is in the tooling allowlist group.
    is_allowlisted_tooling: bool,
    /// Whether the caller is in the rollout allowlist group.
    is_rollout_allowlisted: bool,
}

/// Allowlist authorization shared by every restriction in one source batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AllowlistAuthorization {
    is_allowlisted_tooling: bool,
    is_rollout_allowlisted: bool,
}

impl AllowlistAuthorization {
    fn into_authorization_check_result(self, has_acl_access: bool) -> AuthorizationCheckResult {
        AuthorizationCheckResult {
            has_acl_access,
            is_allowlisted_tooling: self.is_allowlisted_tooling,
            is_rollout_allowlisted: self.is_rollout_allowlisted,
        }
    }
}

impl AuthorizationCheckResult {
    pub(crate) fn new(
        has_acl_access: bool,
        is_allowlisted_tooling: bool,
        is_rollout_allowlisted: bool,
    ) -> Self {
        Self {
            has_acl_access,
            is_allowlisted_tooling,
            is_rollout_allowlisted,
        }
    }

    /// Whether the caller has read authorization through ACLs or allowlists.
    pub(crate) fn has_authorization(&self) -> bool {
        self.has_acl_access || self.is_allowlisted_tooling || self.is_rollout_allowlisted
    }

    pub(crate) fn has_acl_access(&self) -> bool {
        self.has_acl_access
    }

    pub(crate) fn is_allowlisted_tooling(&self) -> bool {
        self.is_allowlisted_tooling
    }

    pub(crate) fn is_rollout_allowlisted(&self) -> bool {
        self.is_rollout_allowlisted
    }
}

/// Path restriction information paired with the caller's authorization result.
#[derive(Clone, Debug, PartialEq)]
pub struct PathRestrictionCheckResult {
    restriction_info: PathRestrictionInfo,
    authorization: AuthorizationCheckResult,
    repo_region_acl: MononokeIdentity,
}

impl PathRestrictionCheckResult {
    pub(crate) fn new(
        restriction_info: PathRestrictionInfo,
        authorization: AuthorizationCheckResult,
        repo_region_acl: MononokeIdentity,
    ) -> Self {
        Self {
            restriction_info,
            authorization,
            repo_region_acl,
        }
    }

    /// Restriction information for the checked path.
    pub fn restriction_info(&self) -> &PathRestrictionInfo {
        &self.restriction_info
    }

    /// Whether the caller has read authorization through ACLs or allowlists.
    pub fn has_authorization(&self) -> bool {
        self.authorization.has_authorization()
    }

    /// Consume this result and return the restriction information.
    pub fn into_restriction_info(self) -> PathRestrictionInfo {
        self.restriction_info
    }
}

/// Manifest restriction information paired with the caller's authorization result.
#[derive(Clone, Debug, PartialEq)]
pub struct ManifestRestrictionCheckResult {
    restriction_info: ManifestRestrictionInfo,
    authorization: AuthorizationCheckResult,
    repo_region_acl: MononokeIdentity,
}

impl ManifestRestrictionCheckResult {
    pub(crate) fn new(
        restriction_info: ManifestRestrictionInfo,
        authorization: AuthorizationCheckResult,
        repo_region_acl: MononokeIdentity,
    ) -> Self {
        Self {
            restriction_info,
            authorization,
            repo_region_acl,
        }
    }

    /// Restriction information for the checked manifest.
    pub fn restriction_info(&self) -> &ManifestRestrictionInfo {
        &self.restriction_info
    }

    /// Whether the caller has read authorization through ACLs or allowlists.
    pub fn has_authorization(&self) -> bool {
        self.authorization.has_authorization()
    }
}

/// Result returned by one restriction source fetch.
///
/// `SourceRestrictionChecks<T>` carries the `SourceRestrictionCheck` bound, so
/// this result is still limited to typed source check results without relying on
/// Rust's non-enforcing type-alias bounds.
pub(crate) type SourceRestrictionResult<T> =
    std::result::Result<SourceRestrictionChecks<T>, SourceRestrictionError>;

/// Successful restriction check results returned by one source.
///
/// The results are shared behind an `Arc` so multiple consumers can observe the
/// same typed checks without cloning every item.
#[derive(Clone)]
pub(crate) struct SourceRestrictionChecks<T: SourceRestrictionCheck> {
    inner: Arc<Vec<T>>,
}

impl<T: SourceRestrictionCheck> SourceRestrictionChecks<T> {
    pub(crate) fn new(checks: Vec<T>) -> Self {
        Self {
            inner: Arc::new(checks),
        }
    }

    pub(crate) fn is_restricted(&self) -> bool {
        !self.inner.is_empty()
    }
}

impl<T: SourceRestrictionCheck> AsRef<[T]> for SourceRestrictionChecks<T> {
    fn as_ref(&self) -> &[T] {
        self.inner.as_slice()
    }
}

/// Shared restriction-source error.
///
/// Source errors may be logged and surfaced after the same fetch completes, so
/// the error is shared while preserving the original error as its source.
#[derive(Clone, Debug)]
pub(crate) struct SourceRestrictionError {
    inner: Arc<anyhow::Error>,
}

impl From<anyhow::Error> for SourceRestrictionError {
    fn from(error: anyhow::Error) -> Self {
        Self {
            inner: Arc::new(error),
        }
    }
}

impl std::fmt::Display for SourceRestrictionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner.as_ref())
    }
}

impl std::error::Error for SourceRestrictionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.inner.chain().nth(1)
    }
}

/// Common read-only view over typed source check results.
///
/// Path and manifest checks keep their concrete restriction-info types, but
/// logging and enforcement need a small common surface to derive aggregate
/// authorization, ACL, and restriction-root fields without copying data into a
/// flattened adapter struct.
pub(crate) trait SourceRestrictionCheck: Clone {
    /// Caller authorization result for this source result.
    fn authorization(&self) -> &AuthorizationCheckResult;
    /// Parsed repo-region ACL associated with this restriction.
    fn repo_region_identity(&self) -> &MononokeIdentity;
    /// Restriction root when the source can report one.
    fn restriction_root(&self) -> Option<&NonRootMPath>;
    /// Whether this check type carries restriction roots independent of source.
    fn reports_restriction_roots() -> bool;
}

impl SourceRestrictionCheck for PathRestrictionCheckResult {
    fn authorization(&self) -> &AuthorizationCheckResult {
        &self.authorization
    }

    fn repo_region_identity(&self) -> &MononokeIdentity {
        &self.repo_region_acl
    }

    fn restriction_root(&self) -> Option<&NonRootMPath> {
        Some(&self.restriction_info().restriction_root)
    }

    fn reports_restriction_roots() -> bool {
        true
    }
}

impl SourceRestrictionCheck for ManifestRestrictionCheckResult {
    fn authorization(&self) -> &AuthorizationCheckResult {
        &self.authorization
    }

    fn repo_region_identity(&self) -> &MononokeIdentity {
        &self.repo_region_acl
    }

    fn restriction_root(&self) -> Option<&NonRootMPath> {
        self.restriction_info().restriction_root.as_ref()
    }

    fn reports_restriction_roots() -> bool {
        false
    }
}

/// Aggregate view over one source's typed restriction checks.
///
/// Logging and enforcement derive authorization, ACL, and restriction-root
/// summaries from this shared type so their source aggregation semantics stay in
/// sync.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceRestrictionSummary {
    authorization: AuthorizationCheckResult,
    repo_region_acls: Vec<String>,
    restriction_roots: Vec<NonRootMPath>,
}

impl SourceRestrictionSummary {
    pub(crate) fn from_checks(checks: &[impl SourceRestrictionCheck]) -> Self {
        let has_acl_access = checks
            .iter()
            .all(|check| check.authorization().has_acl_access());
        let is_allowlisted_tooling = checks
            .iter()
            .any(|check| check.authorization().is_allowlisted_tooling());
        let is_rollout_allowlisted = checks
            .iter()
            .any(|check| check.authorization().is_rollout_allowlisted());
        let repo_region_acls = checks
            .iter()
            .map(|check| check.repo_region_identity().to_string())
            .collect();
        let restriction_roots = checks
            .iter()
            .filter_map(SourceRestrictionCheck::restriction_root)
            .cloned()
            .collect();

        Self {
            authorization: AuthorizationCheckResult::new(
                has_acl_access,
                is_allowlisted_tooling,
                is_rollout_allowlisted,
            ),
            repo_region_acls,
            restriction_roots,
        }
    }

    pub(crate) fn has_authorization(&self) -> bool {
        self.authorization.has_authorization()
    }

    pub(crate) fn has_acl_access(&self) -> bool {
        self.authorization.has_acl_access()
    }

    pub(crate) fn is_allowlisted_tooling(&self) -> bool {
        self.authorization.is_allowlisted_tooling()
    }

    pub(crate) fn is_rollout_allowlisted(&self) -> bool {
        self.authorization.is_rollout_allowlisted()
    }

    pub(crate) fn repo_region_acls(&self) -> &[String] {
        &self.repo_region_acls
    }

    pub(crate) fn restriction_roots(&self) -> &[NonRootMPath] {
        &self.restriction_roots
    }

    pub(crate) fn restriction_root_strings(&self) -> Vec<String> {
        self.restriction_roots
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    pub(crate) fn sorted_repo_region_acls(&self) -> Vec<String> {
        let mut acls = self.repo_region_acls.clone();
        acls.sort();
        acls
    }

    pub(crate) fn sorted_restriction_root_strings(&self) -> Vec<String> {
        let mut paths = self.restriction_root_strings();
        paths.sort();
        paths
    }

    pub(crate) fn into_restriction_check_result(self) -> RestrictionCheckResult {
        build_restriction_check_result(self.has_authorization(), self.restriction_roots)
    }
}

/// Cloneable handle for a spawned source fetch.
#[derive(Clone)]
pub(crate) struct SharedFetchHandle<T: SourceRestrictionCheck> {
    inner: Shared<BoxFuture<'static, SourceRestrictionResult<T>>>,
}

impl<T: SourceRestrictionCheck + Send + Sync + 'static> SharedFetchHandle<T> {
    pub(crate) fn from_join_handle(handle: JoinHandle<Result<Vec<T>>>) -> Self {
        Self::from_future(async move { handle.await.map_err(anyhow::Error::from)? })
    }

    pub(crate) fn from_future(
        fetch: impl Future<Output = Result<Vec<T>>> + Send + 'static,
    ) -> Self {
        let inner = async move {
            fetch
                .await
                .map(SourceRestrictionChecks::new)
                .map_err(SourceRestrictionError::from)
        }
        .boxed()
        .shared();
        Self { inner }
    }

    pub(crate) fn from_result(result: Result<Vec<T>>) -> Self {
        Self::from_future(futures::future::ready(result))
    }

    pub(crate) async fn await_result(&self) -> SourceRestrictionResult<T> {
        self.inner.clone().await
    }
}

/// Request-local result of evaluating enforcement condition sets before
/// fetching restriction data.
///
/// Entry point and request-flag filters can be evaluated from `CoreContext`
/// alone, so callers can skip restriction fetches when no set can match this
/// request. `always_enabled` sets are definite matches. Remaining candidates
/// still need fetched restriction ACLs before enforcement can decide whether
/// the accessed restricted root is in scope.
pub(crate) enum PreFilterResult<'a> {
    NoMatch,
    DefiniteMatch {
        candidates: Vec<&'a EnforcementConditionSet>,
    },
    NeedsFetch {
        candidates: Vec<&'a EnforcementConditionSet>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PreFilterVariant {
    Definite,
    NeedsFetch,
}

/// Evaluate ACL and allowlist authorization for a restricted-path access.
pub(crate) async fn check_authorization(
    ctx: &CoreContext,
    acl_provider: &Arc<dyn AclProvider>,
    acls: &[&MononokeIdentity],
    tooling_allowlist_group: Option<&str>,
    rollout_allowlist_group: Option<&str>,
) -> Result<AuthorizationCheckResult> {
    let allowlist_authorization = check_allowlist_authorization(
        ctx,
        acl_provider,
        tooling_allowlist_group,
        rollout_allowlist_group,
    )
    .await?;
    let has_acl_access = has_read_access_to_repo_region_acls(ctx, acl_provider, acls).await?;
    Ok(allowlist_authorization.into_authorization_check_result(has_acl_access))
}

async fn check_restricted_paths_allowlist_authorization(
    ctx: &CoreContext,
    restricted_paths: &RestrictedPaths,
) -> Result<AllowlistAuthorization> {
    check_allowlist_authorization(
        ctx,
        restricted_paths.acl_provider(),
        restricted_paths.config().tooling_allowlist_group.as_deref(),
        restricted_paths.config().rollout_allowlist_group.as_deref(),
    )
    .await
}

async fn check_allowlist_authorization(
    ctx: &CoreContext,
    acl_provider: &Arc<dyn AclProvider>,
    tooling_allowlist_group: Option<&str>,
    rollout_allowlist_group: Option<&str>,
) -> Result<AllowlistAuthorization> {
    let (is_allowlisted_tooling, is_rollout_allowlisted) = tokio::try_join!(
        check_optional_allowlist_group(ctx, acl_provider, tooling_allowlist_group),
        check_optional_allowlist_group(ctx, acl_provider, rollout_allowlist_group),
    )?;
    Ok(AllowlistAuthorization {
        is_allowlisted_tooling,
        is_rollout_allowlisted,
    })
}

async fn check_optional_allowlist_group(
    ctx: &CoreContext,
    acl_provider: &Arc<dyn AclProvider>,
    group_name: Option<&str>,
) -> Result<bool> {
    match group_name {
        Some(group_name) => is_part_of_group(ctx, acl_provider, group_name).await,
        None => Ok(false),
    }
}

/// Build the legacy restricted-path check result shape.
pub(crate) fn build_restriction_check_result(
    has_authorization: bool,
    restriction_roots: Vec<NonRootMPath>,
) -> RestrictionCheckResult {
    RestrictionCheckResult {
        has_authorization,
        restriction_roots,
    }
}

/// Check restriction-root paths and authorization for one or more paths.
pub(crate) async fn get_path_restriction_root_check(
    restricted_paths: &RestrictedPaths,
    ctx: &CoreContext,
    cs_id: Option<ChangesetId>,
    paths: &[NonRootMPath],
) -> Result<Vec<PathRestrictionCheckResult>> {
    let restriction_info =
        restriction_info::get_path_restriction_root_info(restricted_paths, ctx, cs_id, paths)
            .await?;
    check_path_restriction_infos(ctx, restricted_paths, restriction_info).await
}

/// Check ancestor path restrictions and authorization for one or more paths.
pub(crate) async fn get_path_restriction_check(
    restricted_paths: &RestrictedPaths,
    ctx: &CoreContext,
    cs_id: Option<ChangesetId>,
    paths: &[NonRootMPath],
) -> Result<Vec<PathRestrictionCheckResult>> {
    let restriction_info =
        restriction_info::get_path_restriction_info(restricted_paths, ctx, cs_id, paths).await?;
    check_path_restriction_infos(ctx, restricted_paths, restriction_info).await
}

/// Check manifest restrictions and authorization for one manifest source.
pub(crate) async fn get_manifest_restriction_check(
    restricted_paths: &RestrictedPaths,
    ctx: &CoreContext,
    manifest_id: &ManifestId,
    manifest_type: &ManifestType,
    source: ManifestRestrictionSource,
) -> Result<Vec<ManifestRestrictionCheckResult>> {
    let restriction_info = match source {
        ManifestRestrictionSource::Config => {
            restriction_info::get_manifest_restriction_info_from_config(
                restricted_paths,
                ctx,
                manifest_id,
                manifest_type,
            )
            .await
            .context("find config restrictions for manifest-side check")?
        }
        ManifestRestrictionSource::AclManifest => {
            restriction_info::get_manifest_restriction_info_from_acl_manifest(
                restricted_paths,
                ctx,
                manifest_id,
                manifest_type,
            )
            .await
            .context("find AclManifest restrictions for manifest-side check")?
        }
    };
    check_manifest_restriction_infos(ctx, restricted_paths, restriction_info).await
}

/// Apply the request-local portion of `enforcement_condition_sets`.
///
/// This is intentionally split from restriction ACL matching: request metadata
/// is available before fetching restrictions, but `restriction_acls` can only
/// be compared after the accessed restricted roots are known. Splitting the
/// checks lets enforcement avoid unnecessary fetches for requests that cannot
/// match any condition set while keeping restriction-scoped enforcement precise.
pub(crate) fn pre_filter_condition_sets<'a>(
    ctx: &CoreContext,
    condition_sets: &'a [EnforcementConditionSet],
) -> PreFilterResult<'a> {
    let client_entry_point = ctx
        .metadata()
        .client_request_info()
        .map(|cri| cri.entry_point.to_string());
    let server_side_tenting = ctx.session().server_side_tenting();

    let candidates = condition_sets
        .iter()
        .filter(|set| {
            if !condition_set_has_active_filter(set) {
                return false;
            }

            if set.always_enabled {
                return true;
            }

            let entry_point_matches = set.entry_points.is_empty()
                || client_entry_point.as_ref().is_some_and(|entry_point| {
                    set.entry_points
                        .iter()
                        .any(|candidate| candidate == entry_point)
                });
            entry_point_matches && (!set.require_client_request_flag || server_side_tenting)
        })
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        return PreFilterResult::NoMatch;
    }

    if candidates.iter().any(|set| set.always_enabled) {
        PreFilterResult::DefiniteMatch { candidates }
    } else {
        PreFilterResult::NeedsFetch { candidates }
    }
}

fn condition_set_has_active_filter(set: &EnforcementConditionSet) -> bool {
    set.always_enabled
        || !set.entry_points.is_empty()
        || set.require_client_request_flag
        || !set.restriction_acls.is_empty()
}

pub(crate) fn condition_sets_match_restriction_acls(
    condition_sets: &[&EnforcementConditionSet],
    restriction_acls: &[&MononokeIdentity],
) -> bool {
    condition_sets.iter().any(|set| {
        condition_set_has_active_filter(set)
            && (set.restriction_acls.is_empty()
                || set
                    .restriction_acls
                    .iter()
                    .any(|condition_acl| restriction_acls.contains(&condition_acl)))
    })
}

/// Evaluate whether one authoritative source denies this access.
pub(crate) async fn source_denies_access<'a, T>(
    handle: &SharedFetchHandle<T>,
    candidates: &[&'a EnforcementConditionSet],
    pre_filter_variant: &PreFilterVariant,
) -> Result<bool>
where
    T: SourceRestrictionCheck + Send + Sync + 'static,
{
    let result = handle.await_result().await?;
    let any_match = match pre_filter_variant {
        PreFilterVariant::Definite => true,
        PreFilterVariant::NeedsFetch => {
            let restriction_acls = result
                .as_ref()
                .iter()
                .map(SourceRestrictionCheck::repo_region_identity)
                .collect::<Vec<_>>();
            condition_sets_match_restriction_acls(candidates, &restriction_acls)
        }
    };

    Ok(any_match
        && result
            .as_ref()
            .iter()
            .any(|check| !check.authorization().has_authorization()))
}

/// Combine authoritative source denials using deny-precedence semantics.
///
/// A deny wins over sibling errors so `Both` mode can stay fail-closed once it
/// is enabled, while the first remaining error is surfaced if no source denied.
pub(crate) fn authoritative_sources_deny_access(source_denials: Vec<Result<bool>>) -> Result<bool> {
    if source_denials
        .iter()
        .any(|source_denial| matches!(source_denial, Ok(true)))
    {
        return Ok(true);
    }

    for source_denial in source_denials {
        source_denial?;
    }

    Ok(false)
}

/// Check a path against one selected restriction source.
///
/// Normal callers should use `get_path_restriction_check`, which follows the
/// repository's configured lookup behavior. This helper exists for
/// source-comparison paths that must fetch config and AclManifest independently
/// before logging or selecting an authoritative result.
pub(crate) async fn check_path_restriction_from_source(
    ctx: &CoreContext,
    restricted_paths: &RestrictedPaths,
    path: NonRootMPath,
    source: PathRestrictionSource,
) -> Result<Vec<PathRestrictionCheckResult>> {
    match source {
        PathRestrictionSource::Config => {
            check_config_path_restriction_infos(ctx, restricted_paths, &path).await
        }
        PathRestrictionSource::AclManifest(cs_id) => {
            let restriction_info = restriction_info::get_path_restriction_info_from_acl_manifest(
                restricted_paths,
                ctx,
                cs_id,
                std::slice::from_ref(&path),
            )
            .await
            .context("find AclManifest restrictions for path-side fetch")?;
            check_path_restriction_infos(ctx, restricted_paths, restriction_info).await
        }
    }
}

pub(crate) fn spawn_path_restriction_check(
    ctx: &CoreContext,
    restricted_paths: Arc<RestrictedPaths>,
    path: NonRootMPath,
    source: PathRestrictionSource,
) -> SharedFetchHandle<PathRestrictionCheckResult> {
    let ctx = ctx.clone();
    let handle = mononoke::spawn_task(async move {
        check_path_restriction_from_source(&ctx, &restricted_paths, path, source).await
    });
    SharedFetchHandle::from_join_handle(handle)
}

/// Check a manifest against one selected restriction source.
///
/// Normal callers should use `get_manifest_restriction_check`, which exposes
/// the configured manifest check primitive. This helper exists for
/// source-comparison paths that need separate config and AclManifest results.
pub(crate) async fn check_manifest_restriction_from_source(
    ctx: &CoreContext,
    restricted_paths: &RestrictedPaths,
    manifest_id: ManifestId,
    manifest_type: ManifestType,
    source: ManifestRestrictionSource,
) -> Result<Vec<ManifestRestrictionCheckResult>> {
    get_manifest_restriction_check(restricted_paths, ctx, &manifest_id, &manifest_type, source)
        .await
}

pub(crate) fn spawn_manifest_restriction_check(
    ctx: &CoreContext,
    restricted_paths: Arc<RestrictedPaths>,
    manifest_id: ManifestId,
    manifest_type: ManifestType,
    source: ManifestRestrictionSource,
) -> SharedFetchHandle<ManifestRestrictionCheckResult> {
    let ctx = ctx.clone();
    let handle = mononoke::spawn_task(async move {
        check_manifest_restriction_from_source(
            &ctx,
            &restricted_paths,
            manifest_id,
            manifest_type,
            source,
        )
        .await
    });
    SharedFetchHandle::from_join_handle(handle)
}

pub async fn check_path_restriction_infos(
    ctx: &CoreContext,
    restricted_paths: &RestrictedPaths,
    restriction_info: Vec<PathRestrictionInfo>,
) -> Result<Vec<PathRestrictionCheckResult>> {
    if restriction_info.is_empty() {
        return Ok(vec![]);
    }

    let allowlist_authorization =
        check_restricted_paths_allowlist_authorization(ctx, restricted_paths).await?;
    stream::iter(restriction_info)
        .map(|restriction_info| async move {
            let (authorization, acl) = check_restriction_authorization(
                ctx,
                restricted_paths,
                &restriction_info.repo_region_acl,
                allowlist_authorization,
            )
            .await?;
            Ok(PathRestrictionCheckResult::new(
                restriction_info,
                authorization,
                acl,
            ))
        })
        .buffered(100)
        .try_collect()
        .await
}

pub async fn check_config_path_restriction_infos(
    ctx: &CoreContext,
    restricted_paths: &RestrictedPaths,
    path: &NonRootMPath,
) -> Result<Vec<PathRestrictionCheckResult>> {
    let restriction_info = restricted_paths
        .config()
        .path_acls
        .iter()
        .filter(|(prefix, _)| prefix.is_prefix_of(path))
        .map(|(restriction_root, acl)| {
            let repo_region_acl = acl.to_string();
            (
                PathRestrictionInfo {
                    restriction_root: restriction_root.clone(),
                    request_acl: repo_region_acl.clone(),
                    repo_region_acl,
                },
                acl.clone(),
            )
        })
        .collect::<Vec<_>>();
    if restriction_info.is_empty() {
        return Ok(vec![]);
    }

    let allowlist_authorization =
        check_restricted_paths_allowlist_authorization(ctx, restricted_paths).await?;
    stream::iter(restriction_info)
        .map(|(restriction_info, acl)| async move {
            let authorization = check_restriction_authorization_with_acl(
                ctx,
                restricted_paths,
                &acl,
                allowlist_authorization,
            )
            .await?;
            Ok(PathRestrictionCheckResult::new(
                restriction_info,
                authorization,
                acl,
            ))
        })
        .buffered(100)
        .try_collect()
        .await
}

async fn check_manifest_restriction_infos(
    ctx: &CoreContext,
    restricted_paths: &RestrictedPaths,
    restriction_info: Vec<ManifestRestrictionInfo>,
) -> Result<Vec<ManifestRestrictionCheckResult>> {
    if restriction_info.is_empty() {
        return Ok(vec![]);
    }

    let allowlist_authorization =
        check_restricted_paths_allowlist_authorization(ctx, restricted_paths).await?;
    stream::iter(restriction_info)
        .map(|restriction_info| async move {
            let (authorization, acl) = check_restriction_authorization(
                ctx,
                restricted_paths,
                &restriction_info.repo_region_acl,
                allowlist_authorization,
            )
            .await?;
            Ok(ManifestRestrictionCheckResult::new(
                restriction_info,
                authorization,
                acl,
            ))
        })
        .buffered(100)
        .try_collect()
        .await
}

async fn check_restriction_authorization(
    ctx: &CoreContext,
    restricted_paths: &RestrictedPaths,
    repo_region_acl: &str,
    allowlist_authorization: AllowlistAuthorization,
) -> Result<(AuthorizationCheckResult, MononokeIdentity)> {
    let acl = MononokeIdentity::from_str(repo_region_acl)
        .with_context(|| format!("Failed to parse repo_region_acl {repo_region_acl}"))?;
    let authorization = check_restriction_authorization_with_acl(
        ctx,
        restricted_paths,
        &acl,
        allowlist_authorization,
    )
    .await?;
    Ok((authorization, acl))
}

async fn check_restriction_authorization_with_acl(
    ctx: &CoreContext,
    restricted_paths: &RestrictedPaths,
    acl: &MononokeIdentity,
    allowlist_authorization: AllowlistAuthorization,
) -> Result<AuthorizationCheckResult> {
    let has_acl_access =
        has_read_access_to_repo_region_acls(ctx, restricted_paths.acl_provider(), &[acl]).await?;
    Ok(allowlist_authorization.into_authorization_check_result(has_acl_access))
}

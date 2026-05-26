/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! RL Land Service push diversion logic.
//!
//! When a push targets a repo whose name contains the configured marker
//! (`rl_land_service_repo_prefix` in CommonConfig — historical name; treated
//! as a substring match so nested repos like `oculus/aosp/vendor/oculus` are
//! covered alongside top-level `aosp/...` repos) and the JustKnob
//! `scm/mononoke:divert_aosp_push_to_rl_land_service` is enabled, the Git
//! server diverts branch creates and moves to the RL Land Service. Other
//! ref updates (deletes, tags, non-branch refs) are handled by the normal
//! git server path.
//!
//! The git server calls `submitLand` with a `DirectPushRequest` and then
//! polls `getLandStatus` until the request reaches a terminal state.
//!
//! This module is gated behind `#[cfg(fbcode_build)]` because the
//! AospService Thrift client depends on fbcode-only infrastructure.

use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use aosp_service_clients::errors::AsInvalidRequestException;
use aosp_service_clients::make_AospService;
use aosp_service_services::types::DiffChange;
use aosp_service_services::types::DirectPushRequest;
use aosp_service_services::types::GetStatusRequest;
use aosp_service_services::types::LandResult;
use aosp_service_services::types::LandStatus;
use aosp_service_services::types::SubmitLandRequest;
use aosp_service_services::types::SubmitLandRequestType;
use permission_checker::AclProvider;
use permission_checker::MononokeIdentitySetExt;
use repo_identity::RepoIdentityRef;
use thrift_client::MononokeThriftClient;
use tracing::error;
use tracing::info;

use crate::command::RefUpdate;
use crate::model::RepositoryRequestContext;
use crate::service::GitMappingsStore;
use crate::service::GitObjectStore;

/// The SMC tier name for the RL Land Service.
const RL_LAND_SERVICE_TIER: &str = "scm.grepo.aosp-service.prod";

/// Per-attempt server processing budget for `AospService` calls.
///
/// The AospService SMC tier (`scm.grepo.aosp-service.prod`) does not
/// currently publish a ServiceRouter routing config, so SR clients fall
/// back to a global default of ~140 ms — too short for `submitLand` /
/// `getLandStatus`, which can take minutes for multi-repo CAS bookmark
/// moves with concurrent hook runs. Setting this client-side means our
/// explicit override remains in effect even if the AospService team
/// later publishes a routing config.
const RL_LAND_THRIFT_PROCESSING_TIMEOUT: Duration = Duration::from_secs(300);

/// Total client-side wall-clock budget across all SR retries for a single
/// `AospService` call. Sized to allow at least one retry within the
/// 5-minute per-attempt budget above.
const RL_LAND_THRIFT_OVERALL_TIMEOUT: Duration = Duration::from_secs(600);

/// Check whether this push should be diverted to the RL Land Service.
///
/// Repos whose name *contains* the value configured in
/// `rl_land_service_repo_prefix` (CommonConfig) are diverted when the
/// JustKnob `scm/mononoke:divert_aosp_push_to_rl_land_service` is enabled.
///
/// Note: the CommonConfig field is named `_repo_prefix` for historical
/// reasons but is treated as a substring marker, so both `aosp/foo` and
/// `oculus/aosp/vendor/oculus` match when the configured value is `aosp/`.
/// The field can be renamed in a future schema migration.
pub fn should_divert_to_rl_land_service(
    request_context: &RepositoryRequestContext,
) -> anyhow::Result<bool> {
    let marker = match &request_context
        .repo_configs
        .common
        .rl_land_service_repo_prefix
    {
        Some(p) if !p.is_empty() => p.as_str(),
        _ => return Ok(false),
    };
    let repo_name = request_context.repo.repo_identity().name();
    let divert = repo_name.contains(marker)
        && justknobs::eval(
            "scm/mononoke:divert_aosp_push_to_rl_land_service",
            None,
            Some(repo_name),
        );
    Ok(divert)
}

/// Build a `DiffChange` from a single `RefUpdate`.
///
/// Populates the new `old_git_hash` field from `ref_update.from` for
/// non-create updates, so aosp_service can switch from `set_bookmark` to
/// `move_bookmark` and restore CAS-rebase semantics. For branch creates
/// (`from` is the null OID), `old_git_hash` is `None` and aosp_service
/// keeps the `set_bookmark` form.
fn build_diff_change(repo_name: &str, ref_update: &RefUpdate) -> DiffChange {
    let branch = ref_update
        .ref_name
        .strip_prefix("refs/heads/")
        .unwrap_or(ref_update.ref_name.as_str())
        .to_string();
    let old_git_hash = if ref_update.from.is_null() {
        None
    } else {
        Some(hex::encode(ref_update.from.as_slice()))
    };
    DiffChange {
        project: repo_name.to_string(),
        branch,
        git_hash: hex::encode(ref_update.to.as_slice()),
        original_diff_id: None,
        old_git_hash,
        ..Default::default()
    }
}

/// Whether an emergency push was requested and authorized.
pub enum EmergencyPushStatus {
    /// The x-git-emergency-push pushvar was not set.
    NotRequested,
    /// The pushvar was set and the caller is authorized.
    Authorized,
}

/// The Hipster ACL that controls emergency push access.
const EMERGENCY_PUSH_ACL: &str = "scm_emergency_git_push";

/// Check whether this push is an emergency push and whether the caller
/// is authorized.
///
/// Returns `EmergencyPushStatus::NotRequested` if the pushvar is not set.
/// Returns `EmergencyPushStatus::Authorized` if the pushvar is set and
/// the caller is a member of the emergency push ACL.
/// Returns an error if the pushvar is set but the caller is not authorized.
///
/// The `acl_provider` must be extracted from `State` by the caller before
/// entering a `Send` future, because `State` is not `Sync`.
pub async fn check_emergency_push(
    acl_provider: &Arc<dyn AclProvider>,
    request_context: &RepositoryRequestContext,
) -> anyhow::Result<EmergencyPushStatus> {
    if !request_context.pushvars.emergency_push() {
        return Ok(EmergencyPushStatus::NotRequested);
    }

    let identities = request_context.ctx.metadata().identities();
    let checker = acl_provider
        .group(EMERGENCY_PUSH_ACL)
        .await
        .with_context(|| format!("Failed to load ACL '{}'", EMERGENCY_PUSH_ACL))?;

    if checker.is_member(identities).await {
        info!(
            "Emergency push authorized for repo {} by {}",
            request_context.repo.repo_identity().name(),
            identities.to_string(),
        );
        Ok(EmergencyPushStatus::Authorized)
    } else {
        anyhow::bail!(
            "Emergency push rejected: identities [{}] are not authorized. \
             Request membership in the '{}' ACL to use emergency push.",
            identities.to_string(),
            EMERGENCY_PUSH_ACL,
        )
    }
}

/// Send a best-effort `submitLand` notification to the RL Land Service.
///
/// This is used for emergency pushes: the git push has already succeeded,
/// and we notify the RL Land Service asynchronously so it can update the
/// manifest. If the notification fails, the error is logged but not
/// propagated — the push is already done.
pub async fn fire_and_forget_submit_land(
    ref_updates: &[(RefUpdate, anyhow::Result<()>)],
    request_context: &RepositoryRequestContext,
    service_address: Option<String>,
) {
    let ctx = &request_context.ctx;
    let repo_name = request_context.repo.repo_identity().name().to_string();

    // Build DiffChange items from successful ref updates only.
    let changes: Vec<DiffChange> = ref_updates
        .iter()
        .filter_map(|(ref_update, result)| {
            if result.is_err() {
                return None;
            }
            if !ref_update.ref_name.starts_with("refs/heads/") || ref_update.to.is_null() {
                return None;
            }
            Some(build_diff_change(&repo_name, ref_update))
        })
        .collect();

    if changes.is_empty() {
        info!(
            "Emergency push for repo {}: no branch updates to notify RL Land Service about",
            repo_name,
        );
        return;
    }

    let client_result = if let Some(host_port) = service_address {
        MononokeThriftClient::from_host_port(ctx.fb, host_port, make_AospService)
    } else {
        MononokeThriftClient::from_tier_name(
            ctx.fb,
            RL_LAND_SERVICE_TIER.to_string(),
            make_AospService,
        )
    };

    let client = match client_result {
        Ok(c) => c
            .with_processing_timeout(RL_LAND_THRIFT_PROCESSING_TIMEOUT)
            .with_overall_timeout(RL_LAND_THRIFT_OVERALL_TIMEOUT),
        Err(e) => {
            error!(
                "Emergency push for repo {}: failed to create RL Land Service client: {:#}",
                repo_name, e,
            );
            return;
        }
    };

    let request = SubmitLandRequest {
        changes,
        request_type: SubmitLandRequestType::direct_push(DirectPushRequest {
            is_emergency: true,
            ..Default::default()
        }),
        ..Default::default()
    };

    let service = match client.get_service_client(None, Some(ctx)) {
        Ok(s) => s,
        Err(e) => {
            error!(
                "Emergency push for repo {}: failed to get service client: {:#}",
                repo_name, e,
            );
            return;
        }
    };

    match service.submitLand(&request).await {
        Ok(response) => {
            info!(
                "Emergency push for repo {}: RL Land Service notified, request_id={}",
                repo_name, response.request_id,
            );
        }
        Err(e) => {
            error!(
                "Emergency push for repo {}: RL Land Service submitLand failed (best-effort): {:#}",
                repo_name, e,
            );
        }
    }
}

/// Result of diverting a push to the RL Land Service.
pub struct DiversionResult {
    /// Results for ref updates that were processed by the RL Land Service.
    pub diverted: Vec<(RefUpdate, anyhow::Result<()>)>,
    /// Ref updates that were not diverted (deletes, tags, non-branch refs)
    /// and should be processed by the normal git server path.
    pub remaining: Vec<RefUpdate>,
}

/// Returns true if this ref update is a branch create or move that should
/// be diverted to the RL Land Service.
fn is_divertable_ref(ref_update: &RefUpdate) -> bool {
    // Must not be a content ref (tree/blob pointer).
    if ref_update.is_content() {
        return false;
    }
    // Must be a branch (refs/heads/...).
    if !ref_update.ref_name.starts_with("refs/heads/") {
        return false;
    }
    // Must have a non-null target (create or move, not delete).
    !ref_update.to.is_null()
}

/// Divert branch creates/moves to the RL Land Service.
///
/// Only branch ref updates (refs/heads/...) with a non-null target are
/// diverted. Returns the results for diverted refs and the remaining
/// ref updates that should be processed by the normal git server path.
pub async fn divert_to_rl_land_service(
    ref_updates: Vec<RefUpdate>,
    request_context: Arc<RepositoryRequestContext>,
    _git_bonsai_mapping_store: Arc<GitMappingsStore>,
    _object_store: Arc<GitObjectStore>,
    service_address: Option<String>,
) -> anyhow::Result<DiversionResult> {
    let ctx = &request_context.ctx;
    let repo_name = request_context.repo.repo_identity().name().to_string();

    // Partition ref updates into diverted (branch creates/moves) and
    // remaining (deletes, tags, non-branch refs, content refs).
    let (diverted_refs, remaining_refs): (Vec<_>, Vec<_>) =
        ref_updates.into_iter().partition(is_divertable_ref);

    if diverted_refs.is_empty() {
        return Ok(DiversionResult {
            diverted: vec![],
            remaining: remaining_refs,
        });
    }

    let client = if let Some(host_port) = service_address {
        MononokeThriftClient::from_host_port(ctx.fb, host_port, make_AospService)?
    } else {
        MononokeThriftClient::from_tier_name(
            ctx.fb,
            RL_LAND_SERVICE_TIER.to_string(),
            make_AospService,
        )?
    }
    .with_processing_timeout(RL_LAND_THRIFT_PROCESSING_TIMEOUT)
    .with_overall_timeout(RL_LAND_THRIFT_OVERALL_TIMEOUT);

    // Convert diverted ref updates to DiffChange items.
    let changes: Vec<DiffChange> = diverted_refs
        .iter()
        .map(|ref_update| build_diff_change(&repo_name, ref_update))
        .collect();

    let request = SubmitLandRequest {
        changes,
        request_type: SubmitLandRequestType::direct_push(DirectPushRequest {
            is_emergency: false,
            allow_non_fast_forward_move: request_context.pushvars.allow_non_fast_forward(),
            ..Default::default()
        }),
        ..Default::default()
    };

    info!(
        "Diverting push for repo {} ({} branch ref updates) to RL Land Service, {} refs handled locally",
        repo_name,
        diverted_refs.len(),
        remaining_refs.len(),
    );

    // Submit the land request.
    let service = client.get_service_client(None, Some(ctx))?;
    let submit_response = match service.submitLand(&request).await {
        Ok(resp) => resp,
        Err(e) => {
            // BRANCH_NOT_ENABLED means the RL Land Service doesn't manage
            // this project/branch — fall back to the normal push path.
            if let Some(invalid_req) = e.as_invalid_request_exception() {
                if invalid_req.error_code == "BRANCH_NOT_ENABLED" {
                    info!(
                        "RL Land Service returned BRANCH_NOT_ENABLED for repo {}: {} — falling back to normal push path",
                        repo_name, invalid_req.message,
                    );

                    let mut scuba = request_context.ctx.scuba().clone();
                    scuba.add("log_tag", "rl_land_branch_not_enabled_fallback");
                    scuba.add("repo", repo_name.as_str());
                    scuba.add("ref_count", diverted_refs.len());
                    scuba.add("rl_error_message", invalid_req.message.as_str());
                    scuba.unsampled();
                    scuba.log();

                    let all_refs = diverted_refs.into_iter().chain(remaining_refs).collect();
                    return Ok(DiversionResult {
                        diverted: vec![],
                        remaining: all_refs,
                    });
                }
            }
            return Err(e).context(format!(
                "RL Land Service submitLand failed for repo {}",
                repo_name
            ));
        }
    };

    let request_id = submit_response.request_id;
    info!(
        "RL Land Service accepted push for repo {} with request_id {}",
        repo_name, request_id
    );

    // Poll for completion.
    let poll_interval_secs =
        justknobs::get_as::<u64>("scm/mononoke:rl_land_poll_interval_secs", None).max(1);
    let timeout_secs = justknobs::get_as::<u64>("scm/mononoke:rl_land_timeout_secs", None);
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);

    loop {
        let status_request = GetStatusRequest {
            request_id: request_id.clone(),
            ..Default::default()
        };

        let status_response = service
            .getLandStatus(&status_request)
            .await
            .with_context(|| {
                format!(
                    "RL Land Service getLandStatus failed for repo {}",
                    repo_name
                )
            })?;

        match status_response.status {
            LandStatus::COMPLETED => {
                let diverted_results = match status_response.result {
                    Some(LandResult::success(_)) => {
                        info!(
                            "RL Land Service successfully processed push for repo {}",
                            repo_name
                        );
                        diverted_refs.into_iter().map(|r| (r, Ok(()))).collect()
                    }
                    Some(LandResult::failure(f)) => {
                        let err_str = format!(
                            "RL Land Service push failed for repo {}: {} ({})",
                            repo_name, f.error_message, f.error_code
                        );
                        diverted_refs
                            .into_iter()
                            .map(|r| (r, Err(anyhow::anyhow!(err_str.clone()))))
                            .collect()
                    }
                    Some(LandResult::UnknownField(_)) | None => {
                        let err_str = format!(
                            "RL Land Service completed without result for repo {}",
                            repo_name
                        );
                        diverted_refs
                            .into_iter()
                            .map(|r| (r, Err(anyhow::anyhow!(err_str.clone()))))
                            .collect()
                    }
                };
                return Ok(DiversionResult {
                    diverted: diverted_results,
                    remaining: remaining_refs,
                });
            }
            LandStatus::QUEUED | LandStatus::PROCESSING | LandStatus::UNKNOWN => {
                if Instant::now() > deadline {
                    let err_str = format!(
                        "RL Land Service timed out after {}s for repo {}",
                        timeout_secs, repo_name
                    );
                    let diverted_results = diverted_refs
                        .into_iter()
                        .map(|r| (r, Err(anyhow::anyhow!(err_str.clone()))))
                        .collect();
                    return Ok(DiversionResult {
                        diverted: diverted_results,
                        remaining: remaining_refs,
                    });
                }
                tokio::time::sleep(Duration::from_secs(poll_interval_secs)).await;
            }
            _ => {
                let err_str = format!(
                    "RL Land Service returned unexpected status for repo {}",
                    repo_name
                );
                let diverted_results = diverted_refs
                    .into_iter()
                    .map(|r| (r, Err(anyhow::anyhow!(err_str.clone()))))
                    .collect();
                return Ok(DiversionResult {
                    diverted: diverted_results,
                    remaining: remaining_refs,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use gix_hash::Kind;
    use gix_hash::ObjectId;
    use mononoke_macros::mononoke;

    use super::*;
    use crate::command::RefType;

    fn ref_update(ref_name: &str, from: ObjectId, to: ObjectId) -> RefUpdate {
        RefUpdate {
            ref_name: ref_name.to_string(),
            ref_type: RefType::Standard,
            from,
            to,
        }
    }

    /// Non-create move: `from` SHA must be hex-encoded into `old_git_hash`
    /// so aosp_service can switch to `move_bookmark` and restore CAS-rebase.
    #[mononoke::test]
    fn build_diff_change_populates_old_git_hash_for_move() {
        let from = ObjectId::from_hex(b"1111111111111111111111111111111111111111").unwrap();
        let to = ObjectId::from_hex(b"2222222222222222222222222222222222222222").unwrap();
        let dc = build_diff_change("aosp/test", &ref_update("refs/heads/main", from, to));

        assert_eq!(dc.project, "aosp/test");
        assert_eq!(dc.branch, "main");
        assert_eq!(dc.git_hash, "2222222222222222222222222222222222222222");
        assert_eq!(
            dc.old_git_hash.as_deref(),
            Some("1111111111111111111111111111111111111111"),
            "old_git_hash must be populated from RefUpdate.from for non-creates"
        );
    }

    /// Branch create: `from` is the null OID; `old_git_hash` must be `None`
    /// so aosp_service keeps the `set_bookmark` form.
    #[mononoke::test]
    fn build_diff_change_omits_old_git_hash_for_create() {
        let from = ObjectId::null(Kind::Sha1);
        let to = ObjectId::from_hex(b"3333333333333333333333333333333333333333").unwrap();
        let dc = build_diff_change("aosp/test", &ref_update("refs/heads/feature", from, to));

        assert!(
            dc.old_git_hash.is_none(),
            "old_git_hash must be None on branch create (null from-OID)"
        );
        assert_eq!(dc.git_hash, "3333333333333333333333333333333333333333");
    }

    /// Refs without the `refs/heads/` prefix should retain the full ref
    /// name as the branch — caller is responsible for filtering before
    /// passing in. Verifies the strip-prefix-or-fall-back contract.
    #[mononoke::test]
    fn build_diff_change_keeps_full_ref_name_when_no_heads_prefix() {
        let from = ObjectId::from_hex(b"4444444444444444444444444444444444444444").unwrap();
        let to = ObjectId::from_hex(b"5555555555555555555555555555555555555555").unwrap();
        let dc = build_diff_change("aosp/test", &ref_update("refs/tags/v1", from, to));

        assert_eq!(dc.branch, "refs/tags/v1");
    }
}

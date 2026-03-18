/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! RL Land Service push diversion logic.
//!
//! When a push targets a repo whose name matches the configured prefix
//! (`rl_land_service_repo_prefix` in CommonConfig) and the JustKnob
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

use aosp_service_clients::make_AospService;
use aosp_service_services::types::DiffChange;
use aosp_service_services::types::DirectPushRequest;
use aosp_service_services::types::GetStatusRequest;
use aosp_service_services::types::LandResult;
use aosp_service_services::types::LandStatus;
use aosp_service_services::types::SubmitLandRequest;
use aosp_service_services::types::SubmitLandRequestType;
use repo_identity::RepoIdentityRef;
use thrift_client::MononokeThriftClient;
use tracing::info;

use crate::command::RefUpdate;
use crate::model::RepositoryRequestContext;
use crate::service::GitMappingsStore;
use crate::service::GitObjectStore;

/// The SMC tier name for the RL Land Service.
const RL_LAND_SERVICE_TIER: &str = "scm.grepo.aosp-service.prod";

/// Check whether this push should be diverted to the RL Land Service.
///
/// Repos whose name matches the `rl_land_service_repo_prefix` in
/// CommonConfig are diverted when the JustKnob
/// `scm/mononoke:divert_aosp_push_to_rl_land_service` is enabled.
pub fn should_divert_to_rl_land_service(
    request_context: &RepositoryRequestContext,
) -> anyhow::Result<bool> {
    let prefix = match &request_context
        .repo_configs
        .common
        .rl_land_service_repo_prefix
    {
        Some(p) if !p.is_empty() => p.as_str(),
        _ => return Ok(false),
    };
    let repo_name = request_context.repo.repo_identity().name();
    let divert = repo_name.starts_with(prefix)
        && justknobs::eval(
            "scm/mononoke:divert_aosp_push_to_rl_land_service",
            None,
            Some(repo_name),
        )?;
    Ok(divert)
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
    };

    // Convert diverted ref updates to DiffChange items.
    let changes: Vec<DiffChange> = diverted_refs
        .iter()
        .map(|ref_update| {
            let branch = ref_update
                .ref_name
                .strip_prefix("refs/heads/")
                .unwrap_or(ref_update.ref_name.as_str())
                .to_string();

            DiffChange {
                project: repo_name.clone(),
                branch,
                git_hash: hex::encode(ref_update.to.as_slice()),
                original_diff_id: None,
                ..Default::default()
            }
        })
        .collect();

    let request = SubmitLandRequest {
        changes,
        request_type: SubmitLandRequestType::direct_push(DirectPushRequest {
            is_emergency: false,
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
    let submit_response = service.submitLand(&request).await.map_err(|e| {
        anyhow::anyhow!(
            "RL Land Service submitLand failed for repo {}: {}",
            repo_name,
            e
        )
    })?;

    let request_id = submit_response.request_id;
    info!(
        "RL Land Service accepted push for repo {} with request_id {}",
        repo_name, request_id
    );

    // Poll for completion.
    let poll_interval_secs =
        justknobs::get_as::<u64>("scm/mononoke:rl_land_poll_interval_secs", None)?.max(1);
    let timeout_secs = justknobs::get_as::<u64>("scm/mononoke:rl_land_timeout_secs", None)?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);

    loop {
        let status_request = GetStatusRequest {
            request_id: request_id.clone(),
            ..Default::default()
        };

        let status_response = service.getLandStatus(&status_request).await.map_err(|e| {
            anyhow::anyhow!(
                "RL Land Service getLandStatus failed for repo {}: {}",
                repo_name,
                e
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

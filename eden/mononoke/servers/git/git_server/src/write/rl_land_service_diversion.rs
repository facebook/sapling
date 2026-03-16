/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! RL Land Service push diversion logic.
//!
//! When a push targets a repo whose name matches the JustKnob-configured
//! prefix (`scm/mononoke:rl_land_service_repo_prefix`) and the JustKnob
//! `scm/mononoke:divert_aosp_push_to_rl_land_service` is enabled, the Git
//! server diverts the push to the RL Land Service instead of performing
//! normal bookmark movement. The RL Land Service coordinates atomic
//! cross-repo bookmark movements.
//!
//! This module is gated behind `#[cfg(fbcode_build)]` because the
//! multi_repo_land Thrift client depends on fbcode-only infrastructure.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use multi_repo_land_if::MultipleRepoModifyBookmarksParams;
use multi_repo_land_if::RepoBookmarkModification;
use multi_repo_land_if::RepoBookmarkModificationCreate;
use multi_repo_land_if::RepoBookmarkModificationDelete;
use multi_repo_land_if::RepoBookmarkModificationMove;
use multi_repo_land_if::RepoBookmarkModificationSpec;
use multi_repo_land_service_client::MultiRepoLandServiceClient;
use repo_identity::RepoIdentityRef;
use source_control::CommitId;
use source_control::RepoSpecifier;
use tracing::info;

use crate::command::RefUpdate;
use crate::model::RepositoryRequestContext;
use crate::service::GitMappingsStore;
use crate::service::GitObjectStore;

/// The SMC tier name for the RL Land Service.
/// TODO(rajshar): T256068466 Replace with the actual production tier name once known.
const RL_LAND_SERVICE_TIER: &str = "PLACEHOLDER_RL_LAND_SERVICE_TIER";

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

/// Divert a push to the RL Land Service.
///
/// Converts the git ref updates into Thrift `RepoBookmarkModification`
/// requests and sends them to the RL Land Service for processing. The
/// service handles atomic multi-repo bookmark movement and rebase logic.
///
/// If `service_address` is provided (host:port), connects directly.
/// Otherwise, uses SMC tier lookup.
pub async fn divert_to_rl_land_service(
    ref_updates: Vec<RefUpdate>,
    request_context: Arc<RepositoryRequestContext>,
    _git_bonsai_mapping_store: Arc<GitMappingsStore>,
    _object_store: Arc<GitObjectStore>,
    service_address: Option<String>,
) -> anyhow::Result<Vec<(RefUpdate, anyhow::Result<()>)>> {
    let ctx = &request_context.ctx;
    let repo_name = request_context.repo.repo_identity().name().to_string();

    let client = if let Some(host_port) = service_address {
        MultiRepoLandServiceClient::from_host_port(ctx.fb, host_port)?
    } else {
        MultiRepoLandServiceClient::from_tier_name(ctx.fb, RL_LAND_SERVICE_TIER.to_string())?
    };

    let repo_specifier = RepoSpecifier {
        name: repo_name.clone(),
        ..Default::default()
    };

    // Build pushvars map once, shared across all modifications.
    let pushvars_map: Option<BTreeMap<String, Vec<u8>>> = {
        let m: &HashMap<String, Bytes> = request_context.pushvars.as_ref();
        if m.is_empty() {
            None
        } else {
            Some(m.iter().map(|(k, v)| (k.clone(), v.to_vec())).collect())
        }
    };

    // Convert ref updates to Thrift bookmark modifications, using git
    // commit OIDs directly as CommitId::git values.
    let mut modifications = Vec::with_capacity(ref_updates.len());
    let mut diverted_count = 0usize;
    for ref_update in ref_updates.iter() {
        // Skip content refs (tree/blob pointers) — they are already handled
        // by the uploader and have no bookmark to move.
        if ref_update.is_content() {
            continue;
        }

        let bookmark_name = ref_update
            .ref_name
            .strip_prefix("refs/")
            .unwrap_or(ref_update.ref_name.as_str())
            .to_string();

        let modification_spec = match (ref_update.from.is_null(), ref_update.to.is_null()) {
            (true, false) => {
                // Create: from is null, to is non-null
                RepoBookmarkModificationSpec::create_bookmark(RepoBookmarkModificationCreate {
                    target: CommitId::git(ref_update.to.as_slice().to_vec()),
                    ..Default::default()
                })
            }
            (false, true) => {
                // Delete: from is non-null, to is null
                RepoBookmarkModificationSpec::delete_bookmark(RepoBookmarkModificationDelete {
                    old_target: Some(CommitId::git(ref_update.from.as_slice().to_vec())),
                    ..Default::default()
                })
            }
            (false, false) => {
                // Move: both non-null
                RepoBookmarkModificationSpec::move_bookmark(RepoBookmarkModificationMove {
                    target: CommitId::git(ref_update.to.as_slice().to_vec()),
                    old_target: Some(CommitId::git(ref_update.from.as_slice().to_vec())),
                    allow_non_fast_forward_move: true,
                    ..Default::default()
                })
            }
            (true, true) => {
                anyhow::bail!(
                    "Invalid ref update for bookmark {}: both from and to are null",
                    bookmark_name,
                );
            }
        };

        diverted_count += 1;
        modifications.push(RepoBookmarkModification {
            repo: repo_specifier.clone(),
            bookmark_name,
            modification: modification_spec,
            pushvars: pushvars_map.clone(),
            ..Default::default()
        });
    }

    let params = MultipleRepoModifyBookmarksParams {
        repo_bookmark_modifications: modifications,
        manifest_bookmark_modifications: vec![],
        service_identity: None,
        ..Default::default()
    };

    info!(
        "Diverting push for repo {} ({} ref updates) to RL Land Service",
        repo_name, diverted_count
    );

    match client.multiple_repo_modify_bookmarks(ctx, &params).await {
        Ok(_response) => {
            info!(
                "RL Land Service successfully processed push for repo {}",
                repo_name
            );
            Ok(ref_updates
                .into_iter()
                .map(|ref_update| (ref_update, Ok(())))
                .collect())
        }
        Err(e) => {
            let err_str = format!(
                "RL Land Service push diversion failed for repo {}: {}",
                repo_name,
                e.root_cause()
            );
            Ok(ref_updates
                .into_iter()
                .map(|ref_update| (ref_update, Err(anyhow::anyhow!(err_str.clone()))))
                .collect())
        }
    }
}

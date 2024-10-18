/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! The concrete method implementations.
//!
//! This module provides the concrete implementations for methods - functions
//! taking the Params and returning the Results - to be used by the request worker.
//! This module is not aware of the async nature of those methods. All the token
//! handling, enqueuing and polling should be done by the callers.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use async_requests::types::AsynchronousRequestParams;
use async_requests::types::AsynchronousRequestResult;
use async_requests::types::IntoConfigFormat;
use context::CoreContext;
use ephemeral_blobstore::BubbleId;
use ephemeral_blobstore::RepoEphemeralStore;
use futures::future::BoxFuture;
use futures::Future;
use futures::FutureExt;
use megarepo_api::MegarepoApi;
use megarepo_error::MegarepoError;
use mononoke_api::ChangesetContext;
use mononoke_api::ChangesetSpecifier;
use mononoke_api::Mononoke;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_api::RepoContext;
use mononoke_types::ChangesetId;
use repo_authorization::AuthorizationContext;
#[cfg(fbcode_build)]
use scs_methods::commit_sparse_profile_info::commit_sparse_profile_size_impl;
use scs_methods::from_request::FromRequest;
use scs_methods::specifiers::SpecifierExt;
use source_control as thrift;
use source_control::CommitSpecifier;

#[cfg(not(fbcode_build))]
pub async fn commit_sparse_profile_size_impl(
    ctx: &CoreContext,
    repo: RepoContext<Repo>,
    changeset: ChangesetContext<Repo>,
    profiles: thrift::SparseProfiles,
) -> Result<thrift::CommitSparseProfileSizeResponse, scs_errors::ServiceError> {
    todo!()
}

async fn megarepo_sync_changeset<R: MononokeRepo>(
    ctx: &CoreContext,
    megarepo_api: &MegarepoApi<R>,
    params: thrift::MegarepoSyncChangesetParams,
) -> Result<thrift::MegarepoSyncChangesetResponse, MegarepoError> {
    let source_cs_id = ChangesetId::from_bytes(params.cs_id).map_err(MegarepoError::request)?;
    let target_location =
        ChangesetId::from_bytes(params.target_location).map_err(MegarepoError::request)?;
    let cs_id = megarepo_api
        .sync_changeset(
            ctx,
            source_cs_id,
            params.source_name,
            params.target.into_config_format(&megarepo_api.mononoke())?,
            target_location,
        )
        .await?
        .as_ref()
        .into();
    Ok(thrift::MegarepoSyncChangesetResponse {
        cs_id,
        ..Default::default()
    })
}

async fn megarepo_add_sync_target<R: MononokeRepo>(
    ctx: &CoreContext,
    megarepo_api: &MegarepoApi<R>,
    params: thrift::MegarepoAddTargetParams,
) -> Result<thrift::MegarepoAddTargetResponse, MegarepoError> {
    let config = params.config_with_new_target;
    let mut changesets_to_merge = HashMap::new();
    for (s, cs_id) in params.changesets_to_merge {
        let cs_id = ChangesetId::from_bytes(cs_id).map_err(MegarepoError::request)?;
        changesets_to_merge.insert(s, cs_id);
    }
    let cs_id = megarepo_api
        .add_sync_target(
            ctx,
            config.into_config_format(&megarepo_api.mononoke())?,
            changesets_to_merge,
            params.message,
        )
        .await?
        .as_ref()
        .into();
    Ok(thrift::MegarepoAddTargetResponse {
        cs_id,
        ..Default::default()
    })
}

async fn megarepo_add_branching_sync_target<R: MononokeRepo>(
    ctx: &CoreContext,
    megarepo_api: &MegarepoApi<R>,
    params: thrift::MegarepoAddBranchingTargetParams,
) -> Result<thrift::MegarepoAddBranchingTargetResponse, MegarepoError> {
    let cs_id = megarepo_api
        .add_branching_sync_target(
            ctx,
            params.target.into_config_format(&megarepo_api.mononoke())?,
            ChangesetId::from_bytes(params.branching_point).map_err(MegarepoError::request)?,
            params
                .source_target
                .into_config_format(&megarepo_api.mononoke())?,
        )
        .await?
        .as_ref()
        .into();
    Ok(thrift::MegarepoAddBranchingTargetResponse {
        cs_id,
        ..Default::default()
    })
}

async fn megarepo_change_target_config<R: MononokeRepo>(
    ctx: &CoreContext,
    megarepo_api: &MegarepoApi<R>,
    params: thrift::MegarepoChangeTargetConfigParams,
) -> Result<thrift::MegarepoChangeTargetConfigResponse, MegarepoError> {
    let mut changesets_to_merge = HashMap::new();
    for (s, cs_id) in params.changesets_to_merge {
        let cs_id = ChangesetId::from_bytes(cs_id).map_err(MegarepoError::request)?;
        changesets_to_merge.insert(s, cs_id);
    }
    let target_location =
        ChangesetId::from_bytes(params.target_location).map_err(MegarepoError::request)?;
    let cs_id = megarepo_api
        .change_target_config(
            ctx,
            params.target.into_config_format(&megarepo_api.mononoke())?,
            params.new_version,
            target_location,
            changesets_to_merge,
            params.message,
        )
        .await?
        .as_ref()
        .into();
    Ok(thrift::MegarepoChangeTargetConfigResponse {
        cs_id,
        ..Default::default()
    })
}

async fn megarepo_remerge_source<R: MononokeRepo>(
    ctx: &CoreContext,
    megarepo_api: &MegarepoApi<R>,
    params: thrift::MegarepoRemergeSourceParams,
) -> Result<thrift::MegarepoRemergeSourceResponse, MegarepoError> {
    let remerge_cs_id = ChangesetId::from_bytes(params.cs_id).map_err(MegarepoError::request)?;
    let target_location =
        ChangesetId::from_bytes(params.target_location).map_err(MegarepoError::request)?;
    let cs_id = megarepo_api
        .remerge_source(
            ctx,
            params.source_name,
            remerge_cs_id,
            params.message,
            &params.target.into_config_format(&megarepo_api.mononoke())?,
            target_location,
        )
        .await?
        .as_ref()
        .into();

    Ok(thrift::MegarepoRemergeSourceResponse {
        cs_id,
        ..Default::default()
    })
}

/// Given the request params dispatches the request to the right processing
/// function and returns the computation result. Both successfull computation
/// and error are part of the `AsynchronousRequestResult` structure. We only
/// return `Err` for transient errors, to indicate we should retry.
pub(crate) async fn megarepo_async_request_compute<R: MononokeRepo>(
    ctx: &CoreContext,
    mononoke: Arc<Mononoke<Repo>>,
    megarepo_api: &MegarepoApi<R>,
    params: AsynchronousRequestParams,
) -> Result<AsynchronousRequestResult> {
    match params.into() {
        async_requests_types_thrift::AsynchronousRequestParams::megarepo_add_target_params(params) => {
            Ok(megarepo_add_sync_target(ctx, megarepo_api, params)
                .await
                .map_err(|e| e.into())
                .into())
        }
        async_requests_types_thrift::AsynchronousRequestParams::megarepo_add_branching_target_params(params) => {
            Ok(megarepo_add_branching_sync_target(ctx, megarepo_api, params)
                .await
                .map_err(|e| e.into())
                .into())
        }
        async_requests_types_thrift::AsynchronousRequestParams::megarepo_change_target_params(params) => {
            Ok(megarepo_change_target_config(ctx, megarepo_api, params)
                .await
                .map_err(|e| e.into())
                .into())
        }
        async_requests_types_thrift::AsynchronousRequestParams::megarepo_remerge_source_params(params) => {
            Ok(megarepo_remerge_source(ctx, megarepo_api, params)
                .await
                .map_err(|e| e.into())
                .into())
        }
        async_requests_types_thrift::AsynchronousRequestParams::megarepo_sync_changeset_params(params) => {
            Ok(megarepo_sync_changeset(ctx, megarepo_api, params)
                .await
                .map_err(|e| e.into())
                .into())
        }
        async_requests_types_thrift::AsynchronousRequestParams::async_ping_params(params) => {
            Ok(Ok(thrift::AsyncPingResponse {
                payload: params.payload,
                ..Default::default()
            }).into())
        }
        async_requests_types_thrift::AsynchronousRequestParams::commit_sparse_profile_size_params(params) => {
            let (repo, changeset ) = get_repo_and_changeset(ctx, mononoke, &params.commit).await
                .map_err(|e| anyhow!("error finding changeset: {:?}", e))?;

            Ok(commit_sparse_profile_size_impl(ctx, repo, changeset, params.profiles)
                .await
                .map_err(|e| e.into())
                .into())
        }
        async_requests_types_thrift::AsynchronousRequestParams::UnknownField(union_tag) => {
             bail!(
                "this type of request (AsynchronousRequestParams tag {}) not supported by this worker!", union_tag
             )
        }
    }
}

async fn get_repo_and_changeset(
    ctx: &CoreContext,
    mononoke: Arc<Mononoke<Repo>>,
    commit: &CommitSpecifier,
) -> Result<(RepoContext<Repo>, ChangesetContext<Repo>), scs_errors::ServiceError> {
    let changeset_specifier = ChangesetSpecifier::from_request(&commit.id)?;
    let bubble_fetcher = bubble_fetcher_for_changeset(ctx.clone(), changeset_specifier.clone());
    let repo = repo_impl(ctx.clone(), mononoke, &commit.repo, bubble_fetcher).await?;

    let changeset = repo
        .changeset(changeset_specifier)
        .await?
        .ok_or_else(|| scs_errors::commit_not_found(commit.description()))?;

    Ok((repo, changeset))
}

fn bubble_fetcher_for_changeset(
    ctx: CoreContext,
    specifier: ChangesetSpecifier,
) -> impl FnOnce(RepoEphemeralStore) -> BoxFuture<'static, anyhow::Result<Option<BubbleId>>> {
    move |ephemeral| async move { specifier.bubble_id(&ctx, ephemeral).await }.boxed()
}

async fn repo_impl<F, R>(
    ctx: CoreContext,
    mononoke: Arc<Mononoke<Repo>>,
    repo: &thrift::RepoSpecifier,
    bubble_fetcher: F,
) -> Result<RepoContext<Repo>, scs_errors::ServiceError>
where
    F: FnOnce(RepoEphemeralStore) -> R,
    R: Future<Output = anyhow::Result<Option<BubbleId>>>,
{
    let repo = mononoke
        .repo(ctx, &repo.name)
        .await?
        .ok_or_else(|| scs_errors::repo_not_found(repo.description()))?
        .with_bubble(bubble_fetcher)
        .await?
        .with_authorization_context(AuthorizationContext::new_bypass_access_control())
        .build()
        .await?;
    Ok(repo)
}

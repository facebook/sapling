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

use anyhow::anyhow;
use async_requests::types::AsynchronousRequestParams;
use async_requests::types::AsynchronousRequestResult;
use async_requests::types::IntoConfigFormat;
use async_requests::AsyncRequestsError;
use context::CoreContext;
use megarepo_api::MegarepoApi;
use megarepo_error::MegarepoError;
use mononoke_api::MononokeRepo;
use mononoke_types::ChangesetId;
use source_control as thrift;

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
/// funtion and returns the computation result. This function doesn't return
/// `Result` as both successfull computation and error are part of
/// `AsynchronousRequestResult` structure.
pub(crate) async fn megarepo_async_request_compute<R: MononokeRepo>(
    ctx: &CoreContext,
    megarepo_api: &MegarepoApi<R>,
    params: AsynchronousRequestParams,
) -> AsynchronousRequestResult {
    match params.into() {
        async_requests_types_thrift::AsynchronousRequestParams::megarepo_add_target_params(params) => {
            megarepo_add_sync_target(ctx, megarepo_api, params)
                .await
                .map_err(|e| e.into())
                .into()
        }
        async_requests_types_thrift::AsynchronousRequestParams::megarepo_add_branching_target_params(params) => {
            megarepo_add_branching_sync_target(ctx, megarepo_api, params)
                .await
                .map_err(|e| e.into())
                .into()
        }
        async_requests_types_thrift::AsynchronousRequestParams::megarepo_change_target_params(params) => {
            megarepo_change_target_config(ctx, megarepo_api, params)
                .await
                .map_err(|e| e.into())
                .into()
        }
        async_requests_types_thrift::AsynchronousRequestParams::megarepo_remerge_source_params(params) => {
            megarepo_remerge_source(ctx, megarepo_api, params)
                .await
                .map_err(|e| e.into())
                .into()
        }
        async_requests_types_thrift::AsynchronousRequestParams::megarepo_sync_changeset_params(params) => {
            megarepo_sync_changeset(ctx, megarepo_api, params)
                .await
                .map_err(|e| e.into())
                .into()
        }
        async_requests_types_thrift::AsynchronousRequestParams::UnknownField(union_tag) => {
            Err::<thrift::MegarepoRemergeSourceResponse, _>(AsyncRequestsError::internal(anyhow!(
                "this type of request (AsynchronousRequestParams tag {}) not supported by this worker!", union_tag
            )))
            .into()

        }
    }
}

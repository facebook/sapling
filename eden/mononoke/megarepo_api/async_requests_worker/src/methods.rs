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

use anyhow::anyhow;
use async_requests::types::MegarepoAsynchronousRequestParams;
use async_requests::types::MegarepoAsynchronousRequestResult;
use context::CoreContext;
use megarepo_api::MegarepoApi;
use megarepo_error::MegarepoError;
use mononoke_types::ChangesetId;
use source_control as thrift;
use std::collections::HashMap;

async fn megarepo_sync_changeset(
    ctx: &CoreContext,
    megarepo_api: &MegarepoApi,
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
            params.target,
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

async fn megarepo_add_sync_target(
    ctx: &CoreContext,
    megarepo_api: &MegarepoApi,
    params: thrift::MegarepoAddTargetParams,
) -> Result<thrift::MegarepoAddTargetResponse, MegarepoError> {
    let config = params.config_with_new_target;
    let mut changesets_to_merge = HashMap::new();
    for (s, cs_id) in params.changesets_to_merge {
        let cs_id = ChangesetId::from_bytes(cs_id).map_err(MegarepoError::request)?;
        changesets_to_merge.insert(s, cs_id);
    }
    let cs_id = megarepo_api
        .add_sync_target(ctx, config, changesets_to_merge, params.message)
        .await?
        .as_ref()
        .into();
    Ok(thrift::MegarepoAddTargetResponse {
        cs_id,
        ..Default::default()
    })
}

async fn megarepo_add_branching_sync_target(
    ctx: &CoreContext,
    megarepo_api: &MegarepoApi,
    params: thrift::MegarepoAddBranchingTargetParams,
) -> Result<thrift::MegarepoAddBranchingTargetResponse, MegarepoError> {
    let cs_id = megarepo_api
        .add_branching_sync_target(
            ctx,
            params.target,
            ChangesetId::from_bytes(params.branching_point).map_err(MegarepoError::request)?,
            params.source_target,
        )
        .await?
        .as_ref()
        .into();
    Ok(thrift::MegarepoAddBranchingTargetResponse {
        cs_id,
        ..Default::default()
    })
}

async fn megarepo_change_target_config(
    ctx: &CoreContext,
    megarepo_api: &MegarepoApi,
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
            params.target,
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

async fn megarepo_remerge_source(
    ctx: &CoreContext,
    megarepo_api: &MegarepoApi,
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
            &params.target,
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
/// `MegarepoAsynchronousRequestResult` structure.
pub(crate) async fn megarepo_async_request_compute(
    ctx: &CoreContext,
    megarepo_api: &MegarepoApi,
    params: MegarepoAsynchronousRequestParams,
) -> MegarepoAsynchronousRequestResult {
    match params.into() {
        megarepo_types_thrift::MegarepoAsynchronousRequestParams::megarepo_add_target_params(params) => {
            megarepo_add_sync_target(ctx, megarepo_api, params)
                .await
                .into()
        }
        megarepo_types_thrift::MegarepoAsynchronousRequestParams::megarepo_add_branching_target_params(params) => {
            megarepo_add_branching_sync_target(ctx, megarepo_api, params)
                .await
                .into()
        }
        megarepo_types_thrift::MegarepoAsynchronousRequestParams::megarepo_change_target_params(params) => {
            megarepo_change_target_config(ctx, megarepo_api, params)
                .await
                .into()
        }
        megarepo_types_thrift::MegarepoAsynchronousRequestParams::megarepo_remerge_source_params(params) => {
            megarepo_remerge_source(ctx, megarepo_api, params)
                .await
                .into()
        }
        megarepo_types_thrift::MegarepoAsynchronousRequestParams::megarepo_sync_changeset_params(params) => {
            megarepo_sync_changeset(ctx, megarepo_api, params)
                .await
                .into()
        }
        megarepo_types_thrift::MegarepoAsynchronousRequestParams::UnknownField(union_tag) => {
            Err::<thrift::MegarepoRemergeSourceResponse, _>(MegarepoError::internal(anyhow!(
                "this type of reuqest (MegarepoAsynchronousRequestParams tag {}) not supported by this worker!", union_tag
            )))
            .into()

        }
    }
}

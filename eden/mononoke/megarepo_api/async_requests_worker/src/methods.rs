/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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
use async_requests::types::{
    MegarepoAsynchronousRequestParams, MegarepoAsynchronousRequestResult, Target, ThriftParams,
};
use context::CoreContext;
use megarepo_api::MegarepoApi;
use megarepo_error::MegarepoError;
use mononoke_api::BookmarkName;
use mononoke_types::{ChangesetId, RepositoryId};
use source_control as thrift;
use std::collections::HashMap;

#[allow(dead_code)]
async fn megarepo_sync_changeset(
    ctx: &CoreContext,
    megarepo_api: &MegarepoApi,
    params: thrift::MegarepoSyncChangesetParams,
) -> Result<thrift::MegarepoSyncChangesetResponse, MegarepoError> {
    let target = params.target().clone();
    let source_cs_id = ChangesetId::from_bytes(params.cs_id).map_err(MegarepoError::request)?;
    megarepo_api
        .sync_changeset(ctx, source_cs_id, params.source_name, params.target)
        .await?;
    let cs_id = resolve_current_target_bookmark_value(ctx, megarepo_api, &target).await?;
    Ok(thrift::MegarepoSyncChangesetResponse { cs_id })
}

#[allow(dead_code)]
async fn megarepo_add_sync_target(
    ctx: &CoreContext,
    megarepo_api: &MegarepoApi,
    params: thrift::MegarepoAddTargetParams,
) -> Result<thrift::MegarepoAddTargetResponse, MegarepoError> {
    let target = params.target().clone();
    let config = params.config_with_new_target;
    let mut changesets_to_merge = HashMap::new();
    for (s, cs_id) in params.changesets_to_merge {
        let cs_id = ChangesetId::from_bytes(cs_id).map_err(MegarepoError::request)?;
        changesets_to_merge.insert(s, cs_id);
    }
    megarepo_api
        .add_sync_target(&ctx, config, changesets_to_merge, params.message)
        .await?;

    let cs_id = resolve_current_target_bookmark_value(ctx, megarepo_api, &target).await?;
    Ok(thrift::MegarepoAddTargetResponse { cs_id })
}

/// Given the request params dispatches the request to the right processing
/// funtion and returns the computation result. This function doesn't return
/// `Result` as both successfull computation and error are part of
/// `MegarepoAsynchronousRequestResult` structure.
#[allow(dead_code)]
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
        megarepo_types_thrift::MegarepoAsynchronousRequestParams::megarepo_change_target_params(_params) => {
            Err::<thrift::MegarepoChangeTargetConfigResponse, _>(MegarepoError::internal(anyhow!(
                "change_target is not implemented yet!",
            )))
            .into()
        }
        megarepo_types_thrift::MegarepoAsynchronousRequestParams::megarepo_remerge_source_params(_params) => {
            Err::<thrift::MegarepoRemergeSourceResponse, _>(MegarepoError::internal(anyhow!(
                "remerge_source is not implemented yet!",
            )))
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

async fn resolve_current_target_bookmark_value(
    ctx: &CoreContext,
    megarepo_api: &MegarepoApi,
    target: &Target,
) -> Result<Vec<u8>, MegarepoError> {
    let bookmark = BookmarkName::new(target.bookmark.clone()).map_err(MegarepoError::internal)?;
    let repo_id = RepositoryId::new(target.repo_id as i32);
    let maybe_repo = megarepo_api
        .mononoke()
        .repo_by_id(ctx.clone(), repo_id)
        .await
        .map_err(MegarepoError::internal)?;

    let repo = maybe_repo
        .ok_or_else(|| MegarepoError::request(anyhow!("Repo id {} not found", repo_id)))?;

    let cs_id = repo
        .blob_repo()
        .bookmarks()
        .get(ctx.clone(), &bookmark)
        .await
        .map_err(MegarepoError::internal)?
        .ok_or_else(|| MegarepoError::request(anyhow!("{} bookmark not found")))?;
    Ok(Vec::from(cs_id.as_ref()))
}

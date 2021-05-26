/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_requests::tokens::{MegarepoChangeTargetConfigToken, MegarepoRemergeSourceToken};
use async_requests::types::{ThriftParams, Token};
use bookmarks::BookmarkName;
use context::CoreContext;
use megarepo_config::SyncTargetConfig;
use mononoke_types::{ChangesetId, RepositoryId};
use source_control as thrift;
use std::collections::{HashMap, HashSet};

use crate::errors;
use crate::source_control_impl::SourceControlServiceImpl;

/// A fake token to return on `megarepo_add_sync_target` queries
/// This is a temporary hack, existing because there's no real
/// infrastructure for async calls yet.
/// Current implementation of `megarepo_add_sync_target` is
/// incomplete and can be done synchronously. Later it will
/// become much more expensive and will utilize the async
/// request approach. Still, we want to expose the incomplete
/// version of this call, for our clients to test.
const FAKE_ADD_TARGET_TOKEN: i64 = -36;

impl SourceControlServiceImpl {
    fn verify_repos_by_config(
        &self,
        config: &SyncTargetConfig,
    ) -> Result<(), errors::ServiceError> {
        let known_repo_ids: HashSet<RepositoryId> =
            self.mononoke.known_repo_ids().into_iter().collect();

        let repo_ids_in_cfg = {
            let mut repo_ids_in_cfg = Vec::new();
            repo_ids_in_cfg.push(config.target.repo_id);
            repo_ids_in_cfg.extend(config.sources.iter().map(|src| src.repo_id));
            repo_ids_in_cfg
        };

        for repo_id_in_cfg in repo_ids_in_cfg {
            if !known_repo_ids.contains(&RepositoryId::new(repo_id_in_cfg as i32)) {
                return Err(errors::ServiceError::from(errors::repo_not_found(format!(
                    "{}",
                    repo_id_in_cfg
                ))));
            }
        }

        Ok(())
    }

    pub(crate) async fn megarepo_add_sync_target_config(
        &self,
        ctx: CoreContext,
        params: thrift::MegarepoAddConfigParams,
    ) -> Result<thrift::MegarepoAddConfigResponse, errors::ServiceError> {
        let new_config = params.new_config;
        self.verify_repos_by_config(&new_config)?;
        let megarepo_configs = self.megarepo_api.configs();
        megarepo_configs.add_config_version(ctx, new_config).await?;
        Ok(thrift::MegarepoAddConfigResponse {})
    }

    pub(crate) async fn megarepo_add_sync_target(
        &self,
        ctx: CoreContext,
        params: thrift::MegarepoAddTargetParams,
    ) -> Result<thrift::MegarepoAddTargetToken, errors::ServiceError> {
        let config = params.config_with_new_target;
        let target = config.target.clone();
        self.verify_repos_by_config(&config)?;

        let mut changesets_to_merge = HashMap::new();
        for (s, cs_id) in params.changesets_to_merge {
            let cs_id = ChangesetId::from_bytes(cs_id).map_err(errors::invalid_request)?;
            changesets_to_merge.insert(s, cs_id);
        }

        self.megarepo_api
            .add_sync_target(&ctx, config, changesets_to_merge, params.message)
            .await?;

        // TODO (mateusz, stash, simonfar): stop using the fake token
        let token = thrift::MegarepoAddTargetToken {
            id: FAKE_ADD_TARGET_TOKEN,
            target,
        };

        Ok(token)
    }

    pub(crate) async fn megarepo_add_sync_target_poll(
        &self,
        ctx: CoreContext,
        token: thrift::MegarepoAddTargetToken,
    ) -> Result<thrift::MegarepoAddTargetPollResponse, errors::ServiceError> {
        // TODO (ikostia, stash, mitrandir): stop using the fake token
        if token.id == FAKE_ADD_TARGET_TOKEN {
            let bookmark =
                BookmarkName::new(token.target.bookmark).map_err(errors::invalid_request)?;
            let repo_id = RepositoryId::new(token.target.repo_id as i32);
            let maybe_repo = self.mononoke.repo_by_id(ctx.clone(), repo_id).await?;
            let repo = maybe_repo
                .ok_or_else(|| errors::invalid_request(format!("Repo id{} not found", repo_id)))?;

            let maybe_bookmark_val = repo
                .blob_repo()
                .bookmarks()
                .get(ctx, &bookmark)
                .await
                .map_err(errors::internal_error)?;
            let bookmark_val = maybe_bookmark_val
                .ok_or_else(|| errors::invalid_request("{} bookmark not found"))?;

            Ok(thrift::MegarepoAddTargetPollResponse {
                response: Some(thrift::MegarepoAddTargetResponse {
                    // This is obviously incorrect and should be removed together
                    // with the fake token
                    cs_id: Vec::from(bookmark_val.as_ref()),
                }),
            })
        } else {
            Err(errors::ServiceError::from(errors::not_implemented(
                "megarepo_add_sync_target is not yet implemented",
            )))
        }
    }

    pub(crate) async fn megarepo_change_target_config(
        &self,
        ctx: CoreContext,
        params: thrift::MegarepoChangeTargetConfigParams,
    ) -> Result<thrift::MegarepoChangeConfigToken, errors::ServiceError> {
        let token = self
            .megarepo_api
            .async_method_request_queue(&ctx, params.target())
            .await?
            .enqueue(ctx, params)
            .await
            .map_err(|e| errors::internal_error(format!("Failed to enqueue the request: {}", e)))?;

        Ok(token.into_thrift())
    }

    pub(crate) async fn megarepo_change_target_config_poll(
        &self,
        ctx: CoreContext,
        token: thrift::MegarepoChangeConfigToken,
    ) -> Result<thrift::MegarepoChangeTargetConfigPollResponse, errors::ServiceError> {
        let token = MegarepoChangeTargetConfigToken(token);
        let poll_response = self
            .megarepo_api
            .async_method_request_queue(&ctx, token.target())
            .await?
            .poll(ctx, token)
            .await?;

        Ok(poll_response)
    }

    pub(crate) async fn megarepo_sync_changeset(
        &self,
        ctx: CoreContext,
        params: thrift::MegarepoSyncChangesetParams,
    ) -> Result<thrift::MegarepoSyncChangesetToken, errors::ServiceError> {
        let source_cs_id =
            ChangesetId::from_bytes(params.cs_id).map_err(errors::invalid_request)?;
        self.megarepo_api
            .sync_changeset(
                &ctx,
                source_cs_id,
                params.source_name,
                params.target.clone(),
            )
            .await?;

        let token = thrift::MegarepoSyncChangesetToken {
            id: FAKE_ADD_TARGET_TOKEN,
            target: params.target,
        };

        Ok(token)
    }

    pub(crate) async fn megarepo_sync_changeset_poll(
        &self,
        ctx: CoreContext,
        token: thrift::MegarepoSyncChangesetToken,
    ) -> Result<thrift::MegarepoSyncChangesetPollResponse, errors::ServiceError> {
        if token.id == FAKE_ADD_TARGET_TOKEN {
            let bookmark =
                BookmarkName::new(token.target.bookmark).map_err(errors::invalid_request)?;
            let repo_id = RepositoryId::new(token.target.repo_id as i32);
            let maybe_repo = self.mononoke.repo_by_id(ctx.clone(), repo_id).await?;
            let repo = maybe_repo
                .ok_or_else(|| errors::invalid_request(format!("Repo id{} not found", repo_id)))?;

            let maybe_bookmark_val = repo
                .blob_repo()
                .bookmarks()
                .get(ctx, &bookmark)
                .await
                .map_err(errors::internal_error)?;
            let bookmark_val = maybe_bookmark_val
                .ok_or_else(|| errors::invalid_request("{} bookmark not found"))?;
            Ok(thrift::MegarepoSyncChangesetPollResponse {
                response: Some(thrift::MegarepoSyncChangesetResponse {
                    // TODO(stash, mitrandir, simonfar) - return the actual commit here
                    cs_id: Vec::from(bookmark_val.as_ref()),
                }),
            })
        } else {
            Err(errors::ServiceError::from(errors::not_implemented(
                "megarepo_sync_changeset_poll is not yet implemented",
            )))
        }
    }

    pub(crate) async fn megarepo_remerge_source(
        &self,
        ctx: CoreContext,
        params: thrift::MegarepoRemergeSourceParams,
    ) -> Result<thrift::MegarepoRemergeSourceToken, errors::ServiceError> {
        let token = self
            .megarepo_api
            .async_method_request_queue(&ctx, params.target())
            .await?
            .enqueue(ctx, params)
            .await
            .map_err(|e| errors::internal_error(format!("Failed to enqueue the request: {}", e)))?;

        Ok(token.into_thrift())
    }

    pub(crate) async fn megarepo_remerge_source_poll(
        &self,
        ctx: CoreContext,
        token: thrift::MegarepoRemergeSourceToken,
    ) -> Result<thrift::MegarepoRemergeSourcePollResponse, errors::ServiceError> {
        let token = MegarepoRemergeSourceToken(token);
        let poll_response = self
            .megarepo_api
            .async_method_request_queue(&ctx, token.target())
            .await?
            .poll(ctx, token)
            .await?;

        Ok(poll_response)
    }
}

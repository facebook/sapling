/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Result;
use async_requests::tokens::MegarepoAddBranchingTargetToken;
use async_requests::tokens::MegarepoAddTargetToken;
use async_requests::tokens::MegarepoChangeTargetConfigToken;
use async_requests::tokens::MegarepoRemergeSourceToken;
use async_requests::tokens::MegarepoSyncChangesetToken;
use async_requests::types::IntoApiFormat;
use async_requests::types::IntoConfigFormat;
use context::CoreContext;
use megarepo_config::SyncTargetConfig;
use mononoke_api::ChangesetSpecifier;
use mononoke_api::Mononoke;
use mononoke_api::MononokeError;
use mononoke_api::MononokeRepo;
use mononoke_types::RepositoryId;
use repo_authorization::RepoWriteOperation;
use slog::warn;
use source_control as thrift;

use crate::async_requests::enqueue;
use crate::async_requests::poll;
use crate::from_request::FromRequest;
use crate::source_control_impl::SourceControlServiceImpl;

impl SourceControlServiceImpl {
    fn verify_repos_by_config(
        &self,
        config: &SyncTargetConfig,
    ) -> Result<(), scs_errors::ServiceError> {
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
                return Err(scs_errors::ServiceError::from(scs_errors::repo_not_found(
                    format!("{}", repo_id_in_cfg),
                )));
            }
        }

        Ok(())
    }

    async fn check_write_allowed(
        &self,
        ctx: &CoreContext,
        target_repo_id: RepositoryId,
    ) -> Result<(), scs_errors::ServiceError> {
        let target_repo = self
            .mononoke
            .repo_by_id(ctx.clone(), target_repo_id)
            .await
            .map_err(scs_errors::invalid_request)?
            .ok_or_else(|| {
                scs_errors::invalid_request(anyhow!("repo not found {}", target_repo_id))
            })?
            .build()
            .await?;
        // Check that source control service writes are enabled
        target_repo.start_write()?;
        // Check that we are allowed to write to the target repo
        target_repo
            .authorization_context()
            .require_repo_write(ctx, target_repo.repo(), RepoWriteOperation::MegarepoSync)
            .await
            .map_err(MononokeError::from)?;
        Ok(())
    }

    pub(crate) async fn megarepo_add_sync_target_config(
        &self,
        ctx: CoreContext,
        params: thrift::MegarepoAddConfigParams,
    ) -> Result<thrift::MegarepoAddConfigResponse, scs_errors::ServiceError> {
        let target = params
            .new_config
            .target
            .clone()
            .into_config_format(&self.mononoke)?;
        let target_repo_id = RepositoryId::new(target.repo_id.try_into().unwrap());
        self.check_write_allowed(&ctx, target_repo_id).await?;
        let repo_configs = self.configs.repo_configs();
        let (_, target_repo_config) = repo_configs
            .get_repo_config(target_repo_id)
            .ok_or_else(|| MononokeError::InvalidRequest("repo not found".to_string()))?;

        let new_config = params.new_config.into_config_format(&self.mononoke)?;
        self.verify_repos_by_config(&new_config)?;
        let megarepo_configs = self.megarepo_api.configs();
        megarepo_configs
            .add_config_version(
                ctx.clone(),
                Arc::new(target_repo_config.clone()),
                new_config.clone(),
            )
            .await?;

        // We've seen cases where config is not readable immediately after
        // it was written. Let's try reading a few times before returning
        let mut latest_error = None;
        for _ in 0..10 {
            let res = megarepo_configs
                .get_config_by_version(
                    ctx.clone(),
                    Arc::new(target_repo_config.clone()),
                    new_config.target.clone(),
                    new_config.version.clone(),
                )
                .await;
            match res {
                Ok(_) => {
                    latest_error = None;
                    break;
                }
                Err(err) => {
                    latest_error = Some(err);
                    warn!(
                        ctx.logger(),
                        "failed to read just written config version {}, retrying...",
                        new_config.version
                    );

                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }

        if let Some(err) = latest_error {
            return Err(scs_errors::internal_error(format!(
                "Failed to read just written config version {}, error: {:?}",
                new_config.version, err
            ))
            .into());
        }

        Ok(thrift::MegarepoAddConfigResponse {
            ..Default::default()
        })
    }

    pub(crate) async fn megarepo_read_target_config(
        &self,
        ctx: CoreContext,
        params: thrift::MegarepoReadConfigParams,
    ) -> Result<thrift::MegarepoReadConfigResponse, scs_errors::ServiceError> {
        let target = params.target.clone().into_config_format(&self.mononoke)?;
        let repo = self
            .megarepo_api
            .target_repo(&ctx, &target)
            .await
            .map_err(|err| {
                scs_errors::invalid_request(anyhow!(
                    "can't open target repo {}: {}",
                    target.repo_id,
                    err
                ))
            })?;
        let changeset = repo
            .changeset(ChangesetSpecifier::from_request(&params.commit)?)
            .await?
            .ok_or_else(|| scs_errors::invalid_request(anyhow!("commit not found")))?;
        let (_commit_remapping_state, target_config) = self
            .megarepo_api
            .get_target_sync_config(&ctx, &target, &changeset.id())
            .await?;

        Ok(thrift::MegarepoReadConfigResponse {
            config: target_config.into_api_format(&self.mononoke)?,
            ..Default::default()
        })
    }

    pub(crate) async fn megarepo_add_sync_target(
        &self,
        ctx: CoreContext,
        params: thrift::MegarepoAddTargetParams,
    ) -> Result<thrift::MegarepoAddTargetToken, scs_errors::ServiceError> {
        let target_repo_id =
            get_repo_id_from_target(&params.config_with_new_target.target, &self.mononoke)?;
        self.check_write_allowed(&ctx, target_repo_id).await?;
        let config_with_new_target = params
            .config_with_new_target
            .clone()
            .into_config_format(&self.mononoke)?;
        self.verify_repos_by_config(&config_with_new_target)?;

        enqueue::<thrift::MegarepoAddTargetParams>(
            &ctx,
            &self.async_requests_queue,
            Some(&target_repo_id),
            params,
        )
        .await
    }

    pub(crate) async fn megarepo_add_sync_target_poll(
        &self,
        ctx: CoreContext,
        token: thrift::MegarepoAddTargetToken,
    ) -> Result<thrift::MegarepoAddTargetPollResponse, scs_errors::ServiceError> {
        let token = MegarepoAddTargetToken(token);
        poll::<MegarepoAddTargetToken>(&ctx, &self.async_requests_queue, token).await
    }

    pub(crate) async fn megarepo_add_branching_sync_target(
        &self,
        ctx: CoreContext,
        params: thrift::MegarepoAddBranchingTargetParams,
    ) -> Result<thrift::MegarepoAddBranchingTargetToken, scs_errors::ServiceError> {
        let target_repo_id = get_repo_id_from_target(&params.target, &self.mononoke)?;
        self.check_write_allowed(&ctx, target_repo_id).await?;

        enqueue::<thrift::MegarepoAddBranchingTargetParams>(
            &ctx,
            &self.async_requests_queue,
            Some(&target_repo_id),
            params,
        )
        .await
    }

    pub(crate) async fn megarepo_add_branching_sync_target_poll(
        &self,
        ctx: CoreContext,
        token: thrift::MegarepoAddBranchingTargetToken,
    ) -> Result<thrift::MegarepoAddBranchingTargetPollResponse, scs_errors::ServiceError> {
        let token = MegarepoAddBranchingTargetToken(token);
        poll::<MegarepoAddBranchingTargetToken>(&ctx, &self.async_requests_queue, token).await
    }

    pub(crate) async fn megarepo_change_target_config(
        &self,
        ctx: CoreContext,
        params: thrift::MegarepoChangeTargetConfigParams,
    ) -> Result<thrift::MegarepoChangeConfigToken, scs_errors::ServiceError> {
        let target_repo_id = get_repo_id_from_target(&params.target, &self.mononoke)?;
        self.check_write_allowed(&ctx, target_repo_id).await?;

        enqueue::<thrift::MegarepoChangeTargetConfigParams>(
            &ctx,
            &self.async_requests_queue,
            Some(&target_repo_id),
            params,
        )
        .await
    }

    pub(crate) async fn megarepo_change_target_config_poll(
        &self,
        ctx: CoreContext,
        token: thrift::MegarepoChangeConfigToken,
    ) -> Result<thrift::MegarepoChangeTargetConfigPollResponse, scs_errors::ServiceError> {
        let token = MegarepoChangeTargetConfigToken(token);
        poll::<MegarepoChangeTargetConfigToken>(&ctx, &self.async_requests_queue, token).await
    }

    pub(crate) async fn megarepo_sync_changeset(
        &self,
        ctx: CoreContext,
        params: thrift::MegarepoSyncChangesetParams,
    ) -> Result<thrift::MegarepoSyncChangesetToken, scs_errors::ServiceError> {
        let target_repo_id = get_repo_id_from_target(&params.target, &self.mononoke)?;
        self.check_write_allowed(&ctx, target_repo_id).await?;

        enqueue::<thrift::MegarepoSyncChangesetParams>(
            &ctx,
            &self.async_requests_queue,
            Some(&target_repo_id),
            params,
        )
        .await
    }

    pub(crate) async fn megarepo_sync_changeset_poll(
        &self,
        ctx: CoreContext,
        token: thrift::MegarepoSyncChangesetToken,
    ) -> Result<thrift::MegarepoSyncChangesetPollResponse, scs_errors::ServiceError> {
        let token = MegarepoSyncChangesetToken(token);
        poll::<MegarepoSyncChangesetToken>(&ctx, &self.async_requests_queue, token).await
    }

    pub(crate) async fn megarepo_remerge_source(
        &self,
        ctx: CoreContext,
        params: thrift::MegarepoRemergeSourceParams,
    ) -> Result<thrift::MegarepoRemergeSourceToken, scs_errors::ServiceError> {
        let target_repo_id = get_repo_id_from_target(&params.target, &self.mononoke)?;
        self.check_write_allowed(&ctx, target_repo_id).await?;

        enqueue::<thrift::MegarepoRemergeSourceParams>(
            &ctx,
            &self.async_requests_queue,
            Some(&target_repo_id),
            params,
        )
        .await
    }

    pub(crate) async fn megarepo_remerge_source_poll(
        &self,
        ctx: CoreContext,
        token: thrift::MegarepoRemergeSourceToken,
    ) -> Result<thrift::MegarepoRemergeSourcePollResponse, scs_errors::ServiceError> {
        let token = MegarepoRemergeSourceToken(token);
        poll::<MegarepoRemergeSourceToken>(&ctx, &self.async_requests_queue, token).await
    }
}

/// Retrieve the repo_id from the `target` field of the original Thrift request.
fn get_repo_id_from_target<R: MononokeRepo>(
    target: &thrift::MegarepoTarget,
    mononoke: &Mononoke<R>,
) -> Result<RepositoryId, scs_errors::ServiceError> {
    match (&target.repo, target.repo_id) {
        (Some(repo), _) => {
            let repo = mononoke
                .repo_id_from_name(repo.name.clone())
                .ok_or_else(|| {
                    scs_errors::invalid_request(format!("Invalid repo_name {}", repo.name))
                })?;
            Ok(RepositoryId::new(repo.id()))
        }
        (_, Some(repo_id)) => Ok(RepositoryId::new(
            repo_id.try_into().map_err(scs_errors::invalid_request)?,
        )),
        (None, None) => Err(scs_errors::invalid_request(
            "both repo_id and repo_name are None!",
        ))?,
    }
}

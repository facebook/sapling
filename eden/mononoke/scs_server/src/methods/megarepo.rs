/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use context::CoreContext;
use megarepo_config::SyncTargetConfig;
use mononoke_types::RepositoryId;
use source_control as thrift;
use std::collections::HashSet;

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
const FAKE_ADD_TARGET_TOKEN: &str = "FAKE_ADD_TARGET_TOKEN";

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
        let megarepo_configs = self.mononoke.megarepo_configs();
        megarepo_configs.add_config_version(ctx, new_config).await?;
        Ok(thrift::MegarepoAddConfigResponse {})
    }

    pub(crate) async fn megarepo_add_sync_target(
        &self,
        ctx: CoreContext,
        params: thrift::MegarepoAddTargetParams,
    ) -> Result<thrift::MegarepoAddTargetToken, errors::ServiceError> {
        let config = params.config_with_new_target;
        self.verify_repos_by_config(&config)?;
        // TODO (ikostia): stop using the fake taken
        let megarepo_configs = self.mononoke.megarepo_configs();
        megarepo_configs
            .add_target_with_config_version(ctx, config)
            .await?;

        Ok(FAKE_ADD_TARGET_TOKEN.to_owned())
    }

    pub(crate) async fn megarepo_add_sync_target_poll(
        &self,
        _ctx: CoreContext,
        token: thrift::MegarepoAddTargetToken,
    ) -> Result<thrift::MegarepoAddTargetPollResponse, errors::ServiceError> {
        // TODO (ikostia): stop using the fake taken
        if token == FAKE_ADD_TARGET_TOKEN {
            Ok(thrift::MegarepoAddTargetPollResponse {
                response: Some(thrift::MegarepoAddTargetResponse {}),
            })
        } else {
            Err(errors::ServiceError::from(errors::not_implemented(
                "megarepo_add_sync_target is not yet implemented",
            )))
        }
    }

    pub(crate) async fn megarepo_change_target_config(
        &self,
        _ctx: CoreContext,
        _params: thrift::MegarepoChangeTargetConfigParams,
    ) -> Result<thrift::MegarepoChangeConfigToken, errors::ServiceError> {
        Err(errors::ServiceError::from(errors::not_implemented(
            "megarepo_change_target_config is not yet implemented",
        )))
    }

    pub(crate) async fn megarepo_change_target_config_poll(
        &self,
        _ctx: CoreContext,
        _params: thrift::MegarepoChangeConfigToken,
    ) -> Result<thrift::MegarepoChangeTargetConfigPollResponse, errors::ServiceError> {
        Err(errors::ServiceError::from(errors::not_implemented(
            "poll_megarepo_change_config is not yet implemented",
        )))
    }

    pub(crate) async fn megarepo_sync_changeset(
        &self,
        _ctx: CoreContext,
        _params: thrift::MegarepoSyncChangesetParams,
    ) -> Result<thrift::MegarepoSyncChangesetToken, errors::ServiceError> {
        Err(errors::ServiceError::from(errors::not_implemented(
            "megarepo_sync_changeset is not yet implemented",
        )))
    }

    pub(crate) async fn megarepo_sync_changeset_poll(
        &self,
        _ctx: CoreContext,
        _params: thrift::MegarepoSyncChangesetToken,
    ) -> Result<thrift::MegarepoSyncChangesetPollResponse, errors::ServiceError> {
        Err(errors::ServiceError::from(errors::not_implemented(
            "poll_megarepo_sync_changeset is not yet implemented",
        )))
    }

    pub(crate) async fn megarepo_remerge_source(
        &self,
        _ctx: CoreContext,
        _params: thrift::MegarepoRemergeSourceParams,
    ) -> Result<thrift::MegarepoRemergeSourceToken, errors::ServiceError> {
        Err(errors::ServiceError::from(errors::not_implemented(
            "megarepo_remerge_source is not yet implemented",
        )))
    }

    pub(crate) async fn megarepo_remerge_source_poll(
        &self,
        _ctx: CoreContext,
        _params: thrift::MegarepoRemergeSourceToken,
    ) -> Result<thrift::MegarepoRemergeSourcePollResponse, errors::ServiceError> {
        Err(errors::ServiceError::from(errors::not_implemented(
            "poll_megarepo_remerge_source is not yet implemented",
        )))
    }
}

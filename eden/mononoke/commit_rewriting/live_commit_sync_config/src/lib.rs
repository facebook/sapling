/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{Error, Result};
use cached_config::{ConfigHandle, ConfigStore};
use commitsync::types::{RawCommitSyncAllVersions, RawCommitSyncCurrentVersions};
use context::CoreContext;
use metaconfig_parser::Convert;
use metaconfig_types::CommitSyncConfig;
use mononoke_types::RepositoryId;
use pushredirect_enable::types::{MononokePushRedirectEnable, PushRedirectEnableState};
use slog::{debug, error, info, Logger};
use thiserror::Error;

pub const CONFIGERATOR_PUSHREDIRECT_ENABLE: &str = "scm/mononoke/pushredirect/enable";
pub const CONFIGERATOR_CURRENT_COMMIT_SYNC_CONFIGS: &str =
    "scm/mononoke/repos/commitsyncmaps/current";
pub const CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS: &str = "scm/mononoke/repos/commitsyncmaps/all";

#[derive(Debug, Eq, Error, PartialEq)]
pub enum ErrorKind {
    #[error("{0:?} is not a part of any CommitSyncConfig")]
    NotPartOfAnyCommitSyncConfig(RepositoryId),
    #[error("{0:?} is a part of multiple CommitSyncConfigs")]
    PartOfMultipleCommitSyncConfigs(RepositoryId),
}

#[derive(Clone)]
pub struct LiveCommitSyncConfig {
    config_handle_for_current_versions: ConfigHandle<RawCommitSyncCurrentVersions>,
    config_handle_for_all_versions: ConfigHandle<RawCommitSyncAllVersions>,
    config_handle_for_push_redirection: ConfigHandle<MononokePushRedirectEnable>,
}

impl LiveCommitSyncConfig {
    pub fn new(logger: &Logger, config_store: &ConfigStore) -> Result<Self, Error> {
        info!(logger, "Initializing LiveCommitSyncConfig");
        let config_handle_for_push_redirection =
            config_store.get_config_handle(CONFIGERATOR_PUSHREDIRECT_ENABLE.to_string())?;
        debug!(logger, "Initialized PushRedirect configerator config");
        let config_handle_for_current_versions =
            config_store.get_config_handle(CONFIGERATOR_CURRENT_COMMIT_SYNC_CONFIGS.to_string())?;
        debug!(
            logger,
            "Initialized current commit sync version configerator config"
        );
        let config_handle_for_all_versions =
            config_store.get_config_handle(CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS.to_string())?;
        debug!(
            logger,
            "Initialized all commit sync versions configerator config"
        );
        info!(logger, "Done initializing LiveCommitSyncConfig");
        Ok(Self {
            config_handle_for_current_versions,
            config_handle_for_all_versions,
            config_handle_for_push_redirection,
        })
    }

    fn get_push_redirection_repo_state(
        &self,
        repo_id: RepositoryId,
    ) -> Option<PushRedirectEnableState> {
        let config = self.config_handle_for_push_redirection.get();
        config.per_repo.get(&(repo_id.id() as i64)).cloned()
    }

    pub fn push_redirector_enabled_for_draft(&self, repo_id: RepositoryId) -> bool {
        match self.get_push_redirection_repo_state(repo_id) {
            Some(config) => config.draft_push,
            None => false,
        }
    }

    pub fn push_redirector_enabled_for_public(&self, repo_id: RepositoryId) -> bool {
        match self.get_push_redirection_repo_state(repo_id) {
            Some(config) => config.public_push,
            None => false,
        }
    }

    pub fn get_current_commit_sync_config(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<CommitSyncConfig> {
        let config = self.config_handle_for_current_versions.get();
        let raw_commit_sync_config = {
            let mut interesting_top_level_configs = config
                .repos
                .iter()
                .filter(|(_, commit_sync_config)| {
                    commit_sync_config.large_repo_id == repo_id.id()
                        || commit_sync_config
                            .small_repos
                            .iter()
                            .any(|small_repo| small_repo.repoid == repo_id.id())
                })
                .map(|(_, commit_sync_config)| commit_sync_config);

            let maybe_first = interesting_top_level_configs.next();
            let maybe_second = interesting_top_level_configs.next();
            match (maybe_first, maybe_second) {
                (None, None) => return Err(ErrorKind::NotPartOfAnyCommitSyncConfig(repo_id).into()),
                (Some(raw_config), None) => raw_config.clone(),
                (_, _) => return Err(ErrorKind::PartOfMultipleCommitSyncConfigs(repo_id).into()),
            }
        };

        let commit_sync_config = raw_commit_sync_config.convert()?;

        debug!(
            ctx.logger(),
            "Fetched current commit sync configs: {:?}", commit_sync_config
        );

        Ok(commit_sync_config)
    }
}

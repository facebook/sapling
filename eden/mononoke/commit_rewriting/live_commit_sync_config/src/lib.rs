/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{anyhow, Error, Result};
use cached_config::{ConfigHandle, ConfigStore};
use commitsync::types::{RawCommitSyncAllVersions, RawCommitSyncCurrentVersions};
use context::CoreContext;
use metaconfig_parser::Convert;
use metaconfig_types::{CommitSyncConfig, CommitSyncConfigVersion};
use mononoke_types::RepositoryId;
use pushredirect_enable::types::{MononokePushRedirectEnable, PushRedirectEnableState};
use repos::types::RawCommitSyncConfig;
use slog::{debug, error, info, Logger};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
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
    #[error("Some versions of CommitSyncConfig relate to {0:?}, others don't")]
    OnlySomeVersionsRelateToRepo(RepositoryId),
    #[error("{0:?} is not a part of any CommitSyncConfig version set")]
    NotPartOfAnyCommitSyncConfigVersionSet(RepositoryId),
    #[error("{0:?} is a part of multiple CommitSyncConfigs version sets")]
    PartOfMultipleCommitSyncConfigsVersionSets(RepositoryId),
    #[error("{0:?} has no CommitSyncConfig with version name {1}")]
    UnknownCommitSyncConfigVersion(RepositoryId, String),
}

pub trait LiveCommitSyncConfig: Send + Sync {
    /// Return whether push redirection is currently
    /// enabled for draft commits in `repo_id`
    ///
    /// NOTE: two subsequent calls may return different results
    ///       as this queries  config source
    fn push_redirector_enabled_for_draft(&self, repo_id: RepositoryId) -> bool;

    /// Return whether push redirection is currently
    /// enabled for public commits in `repo_id`
    ///
    /// NOTE: two subsequent calls may return different results
    ///       as this queries  config source
    fn push_redirector_enabled_for_public(&self, repo_id: RepositoryId) -> bool;


    /// Return current version for a given repository
    ///
    /// NOTE: two subsequent calls may return different results
    ///       as this queries  config source
    fn get_current_commit_sync_config_version(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<CommitSyncConfigVersion> {
        let commit_sync_config = self.get_current_commit_sync_config(ctx, repo_id)?;

        let version_name = commit_sync_config.version_name;
        debug!(
            ctx.logger(),
            "Fetched current commit sync config version: {:?}", version_name
        );

        Ok(version_name)
    }

    /// Return current version of `CommitSyncConfig` struct
    /// for a given repository
    ///
    /// NOTE: two subsequent calls may return different results
    ///       as this queries  config source
    fn get_current_commit_sync_config(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<CommitSyncConfig>;

    /// Return all historical versions of `CommitSyncConfig`
    /// structs for a given repository
    ///
    /// NOTE: two subsequent calls may return different results
    ///       as this queries config source
    fn get_all_commit_sync_config_versions(
        &self,
        repo_id: RepositoryId,
    ) -> Result<HashMap<CommitSyncConfigVersion, CommitSyncConfig>>;

    /// Return `CommitSyncConfig` for repo `repo_id` of version `version_name`
    fn get_commit_sync_config_by_version(
        &self,
        repo_id: RepositoryId,
        version_name: &CommitSyncConfigVersion,
    ) -> Result<CommitSyncConfig>;
}

#[derive(Clone)]
pub struct CfgrLiveCommitSyncConfig {
    config_handle_for_current_versions: ConfigHandle<RawCommitSyncCurrentVersions>,
    config_handle_for_all_versions: ConfigHandle<RawCommitSyncAllVersions>,
    config_handle_for_push_redirection: ConfigHandle<MononokePushRedirectEnable>,
}

impl CfgrLiveCommitSyncConfig {
    pub fn new(logger: &Logger, config_store: &ConfigStore) -> Result<Self, Error> {
        info!(logger, "Initializing CfgrLiveCommitSyncConfig");
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
        info!(logger, "Done initializing CfgrLiveCommitSyncConfig");
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

    fn related_to_repo(
        raw_commit_sync_config: &RawCommitSyncConfig,
        repo_id: RepositoryId,
    ) -> bool {
        raw_commit_sync_config.large_repo_id == repo_id.id()
            || raw_commit_sync_config
                .small_repos
                .iter()
                .any(|small_repo| small_repo.repoid == repo_id.id())
    }

    /// Return a clone of the only item in an iterator
    /// Error out otherwise
    fn get_only_item<T: Clone, I: IntoIterator<Item = T>, N: Fn() -> Error, M: Fn() -> Error>(
        items: I,
        no_items_error: N,
        many_items_error: M,
    ) -> Result<T> {
        let mut iter = items.into_iter();
        let maybe_first = iter.next();
        let maybe_second = iter.next();
        match (maybe_first, maybe_second) {
            (None, None) => Err(no_items_error()),
            (Some(only_item), None) => Ok(only_item.clone()),
            (_, _) => return Err(many_items_error()),
        }
    }
}

impl LiveCommitSyncConfig for CfgrLiveCommitSyncConfig {
    /// Return whether push redirection is currently
    /// enabled for draft commits in `repo_id`
    ///
    /// NOTE: two subsequent calls may return different results
    ///       as this queries  config source
    fn push_redirector_enabled_for_draft(&self, repo_id: RepositoryId) -> bool {
        match self.get_push_redirection_repo_state(repo_id) {
            Some(config) => config.draft_push,
            None => false,
        }
    }

    /// Return whether push redirection is currently
    /// enabled for public commits in `repo_id`
    ///
    /// NOTE: two subsequent calls may return different results
    ///       as this queries  config source
    fn push_redirector_enabled_for_public(&self, repo_id: RepositoryId) -> bool {
        match self.get_push_redirection_repo_state(repo_id) {
            Some(config) => config.public_push,
            None => false,
        }
    }

    /// Return current version of `CommitSyncConfig` struct
    /// for a given repository
    ///
    /// NOTE: two subsequent calls may return different results
    ///       as this queries  config source
    fn get_current_commit_sync_config(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<CommitSyncConfig> {
        let config = self.config_handle_for_current_versions.get();
        let raw_commit_sync_config = {
            let interesting_top_level_configs = config
                .repos
                .iter()
                .filter(|(_, raw_commit_sync_config)| {
                    Self::related_to_repo(raw_commit_sync_config, repo_id)
                })
                .map(|(_, commit_sync_config)| commit_sync_config);

            Self::get_only_item(
                interesting_top_level_configs,
                || ErrorKind::NotPartOfAnyCommitSyncConfig(repo_id).into(),
                || ErrorKind::PartOfMultipleCommitSyncConfigs(repo_id).into(),
            )?
            .clone()
        };

        let commit_sync_config = raw_commit_sync_config.convert()?;

        debug!(
            ctx.logger(),
            "Fetched current commit sync configs: {:?}", commit_sync_config
        );

        Ok(commit_sync_config)
    }

    /// Return all historical versions of `CommitSyncConfig`
    /// structs for a given repository
    ///
    /// NOTE: two subsequent calls may return different results
    ///       as this queries config source
    fn get_all_commit_sync_config_versions(
        &self,
        repo_id: RepositoryId,
    ) -> Result<HashMap<CommitSyncConfigVersion, CommitSyncConfig>> {
        let large_repo_config_version_sets = &self.config_handle_for_all_versions.get().repos;

        let mut interesting_configs: Vec<_> = vec![];
        for (_, config_version_set) in large_repo_config_version_sets.iter() {
            for raw_commit_sync_config in config_version_set.versions.iter() {
                if Self::related_to_repo(&raw_commit_sync_config, repo_id) {
                    interesting_configs.push(raw_commit_sync_config.clone());
                }
            }
        }

        let versions: Result<HashMap<CommitSyncConfigVersion, CommitSyncConfig>> =
            interesting_configs
                .into_iter()
                .map(|raw_commit_sync_config| {
                    let commit_sync_config = raw_commit_sync_config.clone().convert()?;
                    let version_name = commit_sync_config.version_name.clone();
                    Ok((version_name, commit_sync_config))
                })
                .collect();

        Ok(versions?)
    }

    /// Return `CommitSyncConfig` for repo `repo_id` of version `version_name`
    fn get_commit_sync_config_by_version(
        &self,
        repo_id: RepositoryId,
        version_name: &CommitSyncConfigVersion,
    ) -> Result<CommitSyncConfig> {
        let mut all_versions = self.get_all_commit_sync_config_versions(repo_id)?;
        all_versions.remove(&version_name).ok_or_else(|| {
            ErrorKind::UnknownCommitSyncConfigVersion(repo_id, version_name.0.clone()).into()
        })
    }
}

/// Inner container for `TestLiveCommitSyncConfigSource`
/// See `TestLiveCommitSyncConfigSource` for more details
struct TestLiveCommitSyncConfigSourceInner {
    version_to_config: Mutex<HashMap<CommitSyncConfigVersion, CommitSyncConfig>>,
    current_versions: Mutex<HashSet<CommitSyncConfigVersion>>,
    push_redirection_for_draft: Mutex<HashMap<RepositoryId, bool>>,
    push_redirection_for_public: Mutex<HashMap<RepositoryId, bool>>,
}

/// A helper type to manage `TestLiveCommitSyncConfig` from outside
/// The idea behind `TestLiveCommitSyncConfig` is that it is going
/// to be used in type-erased contexts, behind `dyn LiveCommitSyncConfig`.
/// Therefore there will be no way to access anything beyond the
/// `LiveCommitSyncConfig` interface, so no way to edit existing config.
/// To allow test scenarios to edit underlying configs, creators of
/// `TestLiveCommitSyncConfig` also receive an accompanying
/// `TestLiveCommitSyncConfigSource`, which allows editing underlying
/// configs
#[derive(Clone)]
pub struct TestLiveCommitSyncConfigSource(Arc<TestLiveCommitSyncConfigSourceInner>);

impl TestLiveCommitSyncConfigSource {
    fn new() -> Self {
        Self(Arc::new(TestLiveCommitSyncConfigSourceInner {
            version_to_config: Mutex::new(HashMap::new()),
            current_versions: Mutex::new(HashSet::new()),
            push_redirection_for_draft: Mutex::new(HashMap::new()),
            push_redirection_for_public: Mutex::new(HashMap::new()),
        }))
    }

    pub fn add_config(&self, config: CommitSyncConfig) {
        self.0
            .version_to_config
            .lock()
            .expect("poisoned lock")
            .insert(config.version_name.clone(), config);
    }

    pub fn add_current_version(&self, version: CommitSyncConfigVersion) {
        self.0
            .current_versions
            .lock()
            .expect("poisoned lock")
            .insert(version);
    }

    pub fn remove_current_version(&self, version: &CommitSyncConfigVersion) {
        self.0
            .current_versions
            .lock()
            .expect("poisoned lock")
            .remove(version);
    }

    pub fn set_draft_push_redirection_enabled(&self, repo_id: RepositoryId) {
        self.0
            .push_redirection_for_draft
            .lock()
            .expect("poisoned lock")
            .insert(repo_id, true);
    }

    pub fn set_public_push_redirection_enabled(&self, repo_id: RepositoryId) {
        self.0
            .push_redirection_for_public
            .lock()
            .expect("poisoned lock")
            .insert(repo_id, true);
    }

    pub fn get_commit_sync_config_for_repo(
        &self,
        repo_id: RepositoryId,
    ) -> Result<CommitSyncConfig> {
        let mut configs = vec![];

        let current_versions = { self.0.current_versions.lock().unwrap().clone() };
        let version_to_config = { self.0.version_to_config.lock().unwrap().clone() };

        for current_version in current_versions {
            match version_to_config.get(&current_version) {
                Some(config) => {
                    let related = Self::related_to_repo(config, repo_id);
                    if related {
                        configs.push(config);
                    }
                }
                None => {
                    return Err(anyhow!("current version {} not found", current_version));
                }
            }
        }

        let mut iter = configs.into_iter();
        match (iter.next(), iter.next()) {
            (Some(config), None) => Ok(config.clone()),
            (Some(first), Some(second)) => Err(anyhow!(
                "too many configs for {}: {:?} and {:?}",
                repo_id,
                first,
                second
            )),
            (None, _) => Err(anyhow!("No config for {}", repo_id)),
        }
    }

    fn push_redirector_enabled_for_draft(&self, repo_id: RepositoryId) -> bool {
        *self
            .0
            .push_redirection_for_draft
            .lock()
            .expect("poisoned lock")
            .get(&repo_id)
            .unwrap_or(&false)
    }

    fn push_redirector_enabled_for_public(&self, repo_id: RepositoryId) -> bool {
        *self
            .0
            .push_redirection_for_public
            .lock()
            .expect("poisoned lock")
            .get(&repo_id)
            .unwrap_or(&false)
    }

    fn get_current_commit_sync_config(
        &self,
        _ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<CommitSyncConfig> {
        self.get_commit_sync_config_for_repo(repo_id)
    }

    fn get_all_commit_sync_config_versions(
        &self,
        repo_id: RepositoryId,
    ) -> Result<HashMap<CommitSyncConfigVersion, CommitSyncConfig>> {
        let version_to_config = { self.0.version_to_config.lock().unwrap().clone() };

        Ok(version_to_config
            .into_iter()
            .filter(|(_, config)| Self::related_to_repo(&config, repo_id))
            .collect())
    }

    fn get_commit_sync_config_by_version(
        &self,
        repo_id: RepositoryId,
        version_name: &CommitSyncConfigVersion,
    ) -> Result<CommitSyncConfig> {
        let config = self
            .0
            .version_to_config
            .lock()
            .unwrap()
            .get(version_name)
            .cloned()
            .ok_or_else(|| anyhow!("{} not found", version_name))?;

        if Self::related_to_repo(&config, repo_id) {
            Ok(config)
        } else {
            Err(anyhow!("{} not found", version_name))
        }
    }

    fn related_to_repo(commit_sync_config: &CommitSyncConfig, repo_id: RepositoryId) -> bool {
        commit_sync_config.large_repo_id == repo_id
            || commit_sync_config
                .small_repos
                .iter()
                .any(|small_repo| small_repo.0 == &repo_id)
    }
}

/// A unit-test freindly implementor of `LiveCommitSyncConfig`
/// As this struct is meant to be held behind a type-erasing
/// `dyn LiveCommitSyncConfig`, anything beyond the interface
/// of `LiveCommitSyncConfig` won't be visible to the users.
/// Therefore, to modify internal state a `TestLiveCommitSyncConfigSource`
/// should be used.
#[derive(Clone)]
pub struct TestLiveCommitSyncConfig {
    source: TestLiveCommitSyncConfigSource,
}

impl TestLiveCommitSyncConfig {
    pub fn new_with_source() -> (Self, TestLiveCommitSyncConfigSource) {
        let source = TestLiveCommitSyncConfigSource::new();
        (
            Self {
                source: source.clone(),
            },
            source,
        )
    }

    pub fn new_empty() -> Self {
        let source = TestLiveCommitSyncConfigSource::new();
        Self { source }
    }
}

impl LiveCommitSyncConfig for TestLiveCommitSyncConfig {
    fn push_redirector_enabled_for_draft(&self, repo_id: RepositoryId) -> bool {
        self.source.push_redirector_enabled_for_draft(repo_id)
    }

    fn push_redirector_enabled_for_public(&self, repo_id: RepositoryId) -> bool {
        self.source.push_redirector_enabled_for_public(repo_id)
    }

    fn get_current_commit_sync_config(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<CommitSyncConfig> {
        self.source.get_current_commit_sync_config(ctx, repo_id)
    }

    fn get_all_commit_sync_config_versions(
        &self,
        repo_id: RepositoryId,
    ) -> Result<HashMap<CommitSyncConfigVersion, CommitSyncConfig>> {
        self.source.get_all_commit_sync_config_versions(repo_id)
    }

    fn get_commit_sync_config_by_version(
        &self,
        repo_id: RepositoryId,
        version_name: &CommitSyncConfigVersion,
    ) -> Result<CommitSyncConfig> {
        self.source
            .get_commit_sync_config_by_version(repo_id, version_name)
    }
}

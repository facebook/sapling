/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use cached_config::ConfigHandle;
use cached_config::ConfigStore;
use commitsync::RawCommitSyncAllVersions;
use commitsync::RawCommitSyncConfigAllVersionsOneRepo;
use context::CoreContext;
use metaconfig_parser::Convert;
use metaconfig_types::CommitSyncConfig;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::CommonCommitSyncConfig;
use mononoke_types::RepositoryId;
use pushredirect::PushRedirectionConfig;
use slog::error;
use thiserror::Error;

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
    #[error("{0:?} is not a part of any configs")]
    NotPartOfAnyConfigs(RepositoryId),
    #[error("{0:?} is a part of multiple config")]
    PartOfMultipleConfigs(RepositoryId),
    #[error("Multiple commit sync config with the same version {0}")]
    MultipleConfigsForSameVersion(CommitSyncConfigVersion),
}

struct PushRedirectEnableState {
    draft_push: bool,
    public_push: bool,
}

#[async_trait]
pub trait LiveCommitSyncConfig: Send + Sync {
    /// Return whether push redirection is currently
    /// enabled for draft commits in `repo_id`
    ///
    /// NOTE: two subsequent calls may return different results
    ///       as this queries  config source
    async fn push_redirector_enabled_for_draft(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<bool>;

    /// Return whether push redirection is currently
    /// enabled for public commits in `repo_id`
    ///
    /// NOTE: two subsequent calls may return different results
    ///       as this queries  config source
    async fn push_redirector_enabled_for_public(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<bool>;

    /// Return all historical versions of `CommitSyncConfig`
    /// structs for a given repository
    ///
    /// NOTE: two subsequent calls may return different results
    ///       as this queries config source
    async fn get_all_commit_sync_config_versions(
        &self,
        repo_id: RepositoryId,
    ) -> Result<HashMap<CommitSyncConfigVersion, CommitSyncConfig>>;

    /// Return `CommitSyncConfig` for repo `repo_id` of version `version_name`
    async fn get_commit_sync_config_by_version(
        &self,
        repo_id: RepositoryId,
        version_name: &CommitSyncConfigVersion,
    ) -> Result<CommitSyncConfig> {
        let maybe_version = self
            .get_commit_sync_config_by_version_if_exists(repo_id, version_name)
            .await?;

        maybe_version.ok_or_else(|| {
            ErrorKind::UnknownCommitSyncConfigVersion(repo_id, version_name.0.clone()).into()
        })
    }

    /// Return `CommitSyncConfig` for repo `repo_id` of version `version_name`
    async fn get_commit_sync_config_by_version_if_exists(
        &self,
        repo_id: RepositoryId,
        version_name: &CommitSyncConfigVersion,
    ) -> Result<Option<CommitSyncConfig>>;

    /// Returns a config that applies to all config versions
    fn get_common_config(&self, repo_id: RepositoryId) -> Result<CommonCommitSyncConfig> {
        self.get_common_config_if_exists(repo_id)?
            .ok_or_else(|| ErrorKind::NotPartOfAnyConfigs(repo_id).into())
    }

    /// Returns a config that applies to all config versions if it exists
    fn get_common_config_if_exists(
        &self,
        repo_id: RepositoryId,
    ) -> Result<Option<CommonCommitSyncConfig>>;
}

#[derive(Clone)]
pub struct CfgrLiveCommitSyncConfig {
    config_handle_for_all_versions: ConfigHandle<RawCommitSyncAllVersions>,
    push_redirect_config: Option<Arc<dyn PushRedirectionConfig>>,
}

impl CfgrLiveCommitSyncConfig {
    pub fn new(
        config_store: &ConfigStore,
        push_redirect_config: Arc<dyn PushRedirectionConfig>,
    ) -> Result<Self, Error> {
        let config_handle_for_all_versions =
            config_store.get_config_handle(CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS.to_string())?;
        Ok(Self {
            config_handle_for_all_versions,
            push_redirect_config: Some(push_redirect_config),
        })
    }

    async fn get_push_redirection_repo_state(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<PushRedirectEnableState> {
        let state_from_xdb = self
            .push_redirect_config
            .clone()
            .expect("push_redirect_config should be available")
            .get(ctx, repo_id)
            .await?;
        let state_from_xdb = state_from_xdb.map_or(
            PushRedirectEnableState {
                draft_push: false,
                public_push: false,
            },
            |cfg| -> PushRedirectEnableState {
                PushRedirectEnableState {
                    draft_push: cfg.draft_push,
                    public_push: cfg.public_push,
                }
            },
        );
        Ok(state_from_xdb)
    }

    fn related_to_repo(
        raw_all_versions: &RawCommitSyncConfigAllVersionsOneRepo,
        repo_id: RepositoryId,
    ) -> bool {
        raw_all_versions.common.large_repo_id == repo_id.id()
            || raw_all_versions
                .common
                .small_repos
                .contains_key(&repo_id.id())
    }
}

#[async_trait]
impl LiveCommitSyncConfig for CfgrLiveCommitSyncConfig {
    /// Return whether push redirection is currently
    /// enabled for draft commits in `repo_id`
    ///
    /// NOTE: two subsequent calls may return different results
    ///       as this queries  config source
    async fn push_redirector_enabled_for_draft(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<bool> {
        Ok(self
            .get_push_redirection_repo_state(ctx, repo_id)
            .await?
            .draft_push)
    }

    /// Return whether push redirection is currently
    /// enabled for public commits in `repo_id`
    ///
    /// NOTE: two subsequent calls may return different results
    ///       as this queries  config source
    async fn push_redirector_enabled_for_public(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<bool> {
        Ok(self
            .get_push_redirection_repo_state(ctx, repo_id)
            .await?
            .public_push)
    }

    /// Return all historical versions of `CommitSyncConfig`
    /// structs for a given repository
    ///
    /// NOTE: two subsequent calls may return different results
    ///       as this queries config source
    async fn get_all_commit_sync_config_versions(
        &self,
        repo_id: RepositoryId,
    ) -> Result<HashMap<CommitSyncConfigVersion, CommitSyncConfig>> {
        let large_repo_config_version_sets = &self.config_handle_for_all_versions.get().repos;

        let mut interesting_configs: Vec<_> = vec![];

        for (_, config_version_set) in large_repo_config_version_sets.iter() {
            if !Self::related_to_repo(config_version_set, repo_id) {
                continue;
            }

            for raw_commit_sync_config in config_version_set.versions.iter() {
                interesting_configs.push(raw_commit_sync_config.clone());
            }
        }

        let versions: Result<HashMap<CommitSyncConfigVersion, CommitSyncConfig>> =
            interesting_configs
                .into_iter()
                .map(|raw_commit_sync_config| {
                    let commit_sync_config = raw_commit_sync_config.convert()?;
                    let version_name = commit_sync_config.version_name.clone();
                    Ok((version_name, commit_sync_config))
                })
                .collect();

        Ok(versions?)
    }

    /// Return `CommitSyncConfig` for repo `repo_id` of version `version_name`
    async fn get_commit_sync_config_by_version_if_exists(
        &self,
        repo_id: RepositoryId,
        version_name: &CommitSyncConfigVersion,
    ) -> Result<Option<CommitSyncConfig>> {
        let large_repo_config_version_sets = &self.config_handle_for_all_versions.get().repos;

        let mut version = None;
        for (_, config_version_set) in large_repo_config_version_sets.iter() {
            if !Self::related_to_repo(config_version_set, repo_id) {
                continue;
            }
            for config in &config_version_set.versions {
                if config.version_name.as_ref() == Some(&version_name.0) {
                    if version.is_some() {
                        return Err(
                            ErrorKind::MultipleConfigsForSameVersion(version_name.clone()).into(),
                        );
                    }
                    version = Some(config.clone().convert()?);
                }
            }
        }

        Ok(version)
    }

    fn get_common_config_if_exists(
        &self,
        repo_id: RepositoryId,
    ) -> Result<Option<CommonCommitSyncConfig>> {
        let config = self.config_handle_for_all_versions.get();
        let maybe_common_config = {
            let interesting_common_configs = config
                .repos
                .iter()
                .filter(|(_, all_versions_config)| {
                    all_versions_config.common.large_repo_id == repo_id.id()
                        || all_versions_config
                            .common
                            .small_repos
                            .contains_key(&repo_id.id())
                })
                .map(|(_, all_versions_config)| all_versions_config.common.clone());

            let mut iter = interesting_common_configs;
            match (iter.next(), iter.next()) {
                (None, _) => Ok(None),
                (Some(config), None) => Ok(Some(config)),
                (Some(_), Some(_)) => Err(ErrorKind::PartOfMultipleConfigs(repo_id)),
            }?
        };
        maybe_common_config.map(Convert::convert).transpose()
    }
}

/// Inner container for `TestLiveCommitSyncConfigSource`
/// See `TestLiveCommitSyncConfigSource` for more details
struct TestLiveCommitSyncConfigSourceInner {
    version_to_config: Mutex<HashMap<CommitSyncConfigVersion, CommitSyncConfig>>,
    push_redirection_for_draft: Mutex<HashMap<RepositoryId, bool>>,
    push_redirection_for_public: Mutex<HashMap<RepositoryId, bool>>,
    common_configs: Mutex<Vec<CommonCommitSyncConfig>>,
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
            push_redirection_for_draft: Mutex::new(HashMap::new()),
            push_redirection_for_public: Mutex::new(HashMap::new()),
            common_configs: Mutex::new(vec![]),
        }))
    }

    pub fn add_config(&self, config: CommitSyncConfig) {
        self.0
            .version_to_config
            .lock()
            .expect("poisoned lock")
            .insert(config.version_name.clone(), config);
    }

    pub fn set_draft_push_redirection_enabled(&self, _ctx: &CoreContext, repo_id: RepositoryId) {
        self.0
            .push_redirection_for_draft
            .lock()
            .expect("poisoned lock")
            .insert(repo_id, true);
    }

    pub fn set_public_push_redirection_enabled(&self, _ctx: &CoreContext, repo_id: RepositoryId) {
        self.0
            .push_redirection_for_public
            .lock()
            .expect("poisoned lock")
            .insert(repo_id, true);
    }

    pub fn add_common_config(&self, config: CommonCommitSyncConfig) {
        self.0
            .common_configs
            .lock()
            .expect("poisoned lock")
            .push(config);
    }

    async fn push_redirector_enabled_for_draft(
        &self,
        _ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<bool> {
        Ok(*self
            .0
            .push_redirection_for_draft
            .lock()
            .expect("poisoned lock")
            .get(&repo_id)
            .unwrap_or(&false))
    }

    async fn push_redirector_enabled_for_public(
        &self,
        _ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<bool> {
        Ok(*self
            .0
            .push_redirection_for_public
            .lock()
            .expect("poisoned lock")
            .get(&repo_id)
            .unwrap_or(&false))
    }

    fn get_all_commit_sync_config_versions(
        &self,
        repo_id: RepositoryId,
    ) -> Result<HashMap<CommitSyncConfigVersion, CommitSyncConfig>> {
        let version_to_config = { self.0.version_to_config.lock().unwrap().clone() };

        Ok(version_to_config
            .into_iter()
            .filter(|(_, config)| Self::related_to_repo(config, repo_id))
            .collect())
    }

    pub fn get_commit_sync_config_by_version_if_exists(
        &self,
        repo_id: RepositoryId,
        version_name: &CommitSyncConfigVersion,
    ) -> Result<Option<CommitSyncConfig>> {
        let maybe_config = self
            .0
            .version_to_config
            .lock()
            .unwrap()
            .get(version_name)
            .cloned();

        let config = match maybe_config {
            Some(config) => config,
            None => {
                return Ok(None);
            }
        };

        if Self::related_to_repo(&config, repo_id) {
            Ok(Some(config))
        } else {
            Err(anyhow!("{} not found", version_name))
        }
    }

    pub fn get_common_config_if_exists(
        &self,
        repo_id: RepositoryId,
    ) -> Result<Option<CommonCommitSyncConfig>> {
        let common_configs = self.0.common_configs.lock().unwrap();
        for config in common_configs.iter() {
            if config.large_repo_id == repo_id || config.small_repos.contains_key(&repo_id) {
                return Ok(Some(config.clone()));
            }
        }

        Ok(None)
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

#[async_trait]
impl LiveCommitSyncConfig for TestLiveCommitSyncConfig {
    async fn push_redirector_enabled_for_draft(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<bool> {
        self.source
            .push_redirector_enabled_for_draft(ctx, repo_id)
            .await
    }

    async fn push_redirector_enabled_for_public(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<bool> {
        self.source
            .push_redirector_enabled_for_public(ctx, repo_id)
            .await
    }

    async fn get_all_commit_sync_config_versions(
        &self,
        repo_id: RepositoryId,
    ) -> Result<HashMap<CommitSyncConfigVersion, CommitSyncConfig>> {
        self.source.get_all_commit_sync_config_versions(repo_id)
    }

    async fn get_commit_sync_config_by_version_if_exists(
        &self,
        repo_id: RepositoryId,
        version_name: &CommitSyncConfigVersion,
    ) -> Result<Option<CommitSyncConfig>> {
        self.source
            .get_commit_sync_config_by_version_if_exists(repo_id, version_name)
    }

    fn get_common_config_if_exists(
        &self,
        repo_id: RepositoryId,
    ) -> Result<Option<CommonCommitSyncConfig>> {
        self.source.get_common_config_if_exists(repo_id)
    }
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(async_closure)]

use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::sync::Arc;

use add_branching_sync_target::AddBranchingSyncTarget;
use add_sync_target::AddSyncTarget;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Error;
use async_once_cell::AsyncOnceCell;
use async_requests::AsyncMethodRequestQueue;
use blobstore::Blobstore;
use change_target_config::ChangeTargetConfig;
use context::CoreContext;
use futures::future::try_join_all;
use megarepo_config::CfgrMononokeMegarepoConfigs;
use megarepo_config::MononokeMegarepoConfigs;
use megarepo_config::MononokeMegarepoConfigsOptions;
use megarepo_config::SyncConfigVersion;
use megarepo_config::SyncTargetConfig;
use megarepo_config::Target;
use megarepo_config::TestMononokeMegarepoConfigs;
use megarepo_error::MegarepoError;
use megarepo_mapping::CommitRemappingState;
use megarepo_mapping::MegarepoMapping;
use megarepo_mapping::SourceName;
use metaconfig_types::ArcRepoConfig;
use metaconfig_types::RepoConfigArc;
use mononoke_api::Mononoke;
use mononoke_api::RepoContext;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mutable_renames::MutableRenames;
use parking_lot::Mutex;
use remerge_source::RemergeSource;
use repo_authorization::AuthorizationContext;
use repo_blobstore::RepoBlobstoreArc;
use repo_factory::RepoFactory;
use repo_identity::ArcRepoIdentity;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityArc;
use requests_table::LongRunningRequestsQueue;
use slog::info;
use slog::o;
use slog::warn;

mod add_branching_sync_target;
#[cfg(test)]
mod add_branching_sync_target_test;
mod add_sync_target;
#[cfg(test)]
mod add_sync_target_test;
mod change_target_config;
#[cfg(test)]
mod change_target_config_test;
mod common;
#[cfg(test)]
mod megarepo_test_utils;
mod remerge_source;
#[cfg(test)]
mod remerge_source_test;
mod sync_changeset;

/// A cache for AsyncMethodRequestQueue instances
#[derive(Clone)]
struct Cache<K: Clone + Eq + Hash, V: Clone> {
    cache: Arc<Mutex<HashMap<K, Arc<AsyncOnceCell<V>>>>>,
}

impl<K: Clone + Eq + Hash, V: Clone> Cache<K, V> {
    fn new() -> Self {
        Cache {
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn get_or_try_init<F, Fut>(&self, key: &K, init: F) -> Result<V, Error>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<V, Error>>,
    {
        let cell = {
            let mut cache = self.cache.lock();
            match cache.get(key) {
                Some(cell) => {
                    if let Some(value) = cell.get() {
                        return Ok(value.clone());
                    }
                    cell.clone()
                }
                None => {
                    let cell = Arc::new(AsyncOnceCell::new());
                    cache.insert(key.clone(), cell.clone());
                    cell
                }
            }
        };
        let value = cell.get_or_try_init(init).await?;
        Ok(value.clone())
    }
}

pub struct MegarepoApi {
    megarepo_configs: Arc<dyn MononokeMegarepoConfigs>,
    queue_cache: Cache<ArcRepoIdentity, AsyncMethodRequestQueue>,
    megarepo_mapping_cache: Cache<ArcRepoIdentity, Arc<MegarepoMapping>>,
    mononoke: Arc<Mononoke>,
    repo_factory: Arc<RepoFactory>,
}

impl MegarepoApi {
    pub async fn new(app: &MononokeApp, mononoke: Arc<Mononoke>) -> Result<Self, MegarepoError> {
        let env = app.environment();
        let fb = env.fb;
        let logger = env.logger.new(o!("megarepo" => ""));

        let megarepo_configs: Arc<dyn MononokeMegarepoConfigs> = match &env.megarepo_configs_options
        {
            MononokeMegarepoConfigsOptions::Prod => Arc::new(CfgrMononokeMegarepoConfigs::new(
                fb,
                &logger,
                env.config_store.clone(),
                None,
            )?),
            MononokeMegarepoConfigsOptions::IntegrationTest(path) => {
                Arc::new(CfgrMononokeMegarepoConfigs::new(
                    fb,
                    &logger,
                    env.config_store.clone(),
                    Some(path.clone()),
                )?)
            }
            MononokeMegarepoConfigsOptions::UnitTest => {
                Arc::new(TestMononokeMegarepoConfigs::new(&logger))
            }
        };

        let repo_factory = app.repo_factory().clone();

        Ok(Self {
            megarepo_configs,
            queue_cache: Cache::new(),
            megarepo_mapping_cache: Cache::new(),
            mononoke,
            repo_factory,
        })
    }

    /// Get megarepo configs
    pub fn configs(&self) -> &dyn MononokeMegarepoConfigs {
        self.megarepo_configs.as_ref()
    }

    /// Get megarepo config and remapping state for given commit in target
    pub async fn get_target_sync_config(
        &self,
        ctx: &CoreContext,
        target: &Target,
        cs_id: &ChangesetId,
    ) -> Result<(CommitRemappingState, SyncTargetConfig), MegarepoError> {
        let target_repo = self.target_repo(ctx, target).await?;
        common::find_target_sync_config(
            ctx,
            target_repo.blob_repo(),
            *cs_id,
            target,
            &self.megarepo_configs,
        )
        .await
    }

    /// Get mononoke object
    pub fn mononoke(&self) -> Arc<Mononoke> {
        self.mononoke.clone()
    }

    /// Get mutable_renames object
    pub async fn mutable_renames(
        &self,
        ctx: &CoreContext,
        target: &Target,
    ) -> Result<Arc<MutableRenames>, MegarepoError> {
        let repo = self.target_repo(ctx, target).await?;
        Ok(repo.mutable_renames())
    }

    /// Get an `AsyncMethodRequestQueue` for a given target
    pub async fn async_method_request_queue(
        &self,
        ctx: &CoreContext,
        target: &Target,
    ) -> Result<AsyncMethodRequestQueue, MegarepoError> {
        let (repo_config, repo_identity) = self.target_repo_config_and_id(ctx, target).await?;
        self.async_method_request_queue_for_repo(ctx, &repo_identity, &repo_config)
            .await
    }

    /// Get an `AsyncMethodRequestQueue` for a given repo
    pub async fn async_method_request_queue_for_repo(
        &self,
        ctx: &CoreContext,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
    ) -> Result<AsyncMethodRequestQueue, MegarepoError> {
        let queue = self
            .queue_cache
            .get_or_try_init(&repo_identity.clone(), || async move {
                let table = self.requests_table(ctx, repo_identity, repo_config).await?;
                let blobstore = self.blobstore(repo_identity.clone()).await?;
                Ok(AsyncMethodRequestQueue::new(table, blobstore))
            })
            .await?;
        Ok(queue)
    }

    /// Get all queues used by configured repos.
    pub async fn all_async_method_request_queues(
        &self,
        ctx: &CoreContext,
    ) -> Result<Vec<(Vec<RepositoryId>, AsyncMethodRequestQueue)>, MegarepoError> {
        // TODO(mitrandir): instead of creating a queue per repo create a queue
        // per group of repos with exactly same storage configs. This way even with
        // a lot of repos we'll have few queues.
        let queues = try_join_all(self.mononoke.repos().map(|repo| {
            let repo_id = repo.repoid();
            let repo_identity = RepoIdentity::new(repo_id, repo.name().to_string());
            let repo_config = repo.config().clone();
            async move {
                let queue = self
                    .async_method_request_queue_for_repo(
                        ctx,
                        &Arc::new(repo_identity),
                        &Arc::new(repo_config),
                    )
                    .await?;
                Ok::<_, MegarepoError>((vec![repo_id], queue))
            }
        }))
        .await?;
        Ok(queues)
    }

    /// Build an instance of `LongRunningRequestsQueue` to be embedded
    /// into `AsyncMethodRequestQueue`.
    async fn requests_table(
        &self,
        ctx: &CoreContext,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
    ) -> Result<Arc<dyn LongRunningRequestsQueue>, Error> {
        info!(
            ctx.logger(),
            "Opening a long_running_requests_queue table for {}",
            repo_identity.name()
        );
        let table = self
            .repo_factory
            .long_running_requests_queue(repo_config)
            .await?;
        info!(
            ctx.logger(),
            "Done opening a long_running_requests_queue table for {}",
            repo_identity.name()
        );

        Ok(table)
    }

    /// Get Mononoke repo config and identity by a target
    async fn target_repo_config_and_id(
        &self,
        ctx: &CoreContext,
        target: &Target,
    ) -> Result<(ArcRepoConfig, ArcRepoIdentity), Error> {
        let repo_id: i32 = TryFrom::<i64>::try_from(target.repo_id)?;
        let repo = match self.mononoke.raw_repo_by_id(repo_id) {
            Some(repo) => repo,
            None => {
                warn!(
                    ctx.logger(),
                    "Unknown repo: {} in target {:?}", repo_id, target
                );
                bail!("unknown repo in the target: {}", repo_id)
            }
        };
        Ok((repo.repo_config_arc(), repo.repo_identity_arc()))
    }

    /// Get Mononoke repo context by terget
    pub async fn target_repo(
        &self,
        ctx: &CoreContext,
        target: &Target,
    ) -> Result<RepoContext, Error> {
        let repo_id: i32 = TryFrom::<i64>::try_from(target.repo_id)?;
        let repo_id = RepositoryId::new(repo_id);
        let repo = self
            .mononoke
            .repo_by_id(ctx.clone(), repo_id)
            .await
            .map_err(MegarepoError::internal)?
            .ok_or_else(|| MegarepoError::request(anyhow!("repo not found {}", repo_id)))?
            .with_authorization_context(AuthorizationContext::new_bypass_access_control())
            .build()
            .await?;
        Ok(repo)
    }

    /// Build a blobstore to be embedded into `AsyncMethodRequestQueue`
    async fn blobstore(&self, repo_identity: ArcRepoIdentity) -> Result<Arc<dyn Blobstore>, Error> {
        let repo = self
            .mononoke
            .raw_repo(repo_identity.name())
            .ok_or_else(|| {
                MegarepoError::request(anyhow!("repo not found {}", repo_identity.name()))
            })?;
        Ok(repo.repo_blobstore_arc())
    }

    /// Build MegarepoMapping
    async fn megarepo_mapping(
        &self,
        ctx: &CoreContext,
        target: &Target,
    ) -> Result<Arc<MegarepoMapping>, Error> {
        let (repo_config, repo_identity) = self.target_repo_config_and_id(ctx, target).await?;

        let megarepo_mapping = self
            .megarepo_mapping_cache
            .get_or_try_init(&repo_identity.clone(), || async move {
                let megarepo_mapping = self
                    .repo_factory
                    .sql_factory(&repo_config.storage_config.metadata)
                    .await?
                    .open::<MegarepoMapping>()?;

                Ok(Arc::new(megarepo_mapping))
            })
            .await?;

        Ok(megarepo_mapping)
    }

    fn prepare_ctx(
        &self,
        ctx: &CoreContext,
        target: Target,
        version: Option<SyncConfigVersion>,
        method: &str,
    ) -> CoreContext {
        ctx.with_mutated_scuba(|mut scuba| {
            scuba.add("target_repo_id", target.repo_id);
            scuba.add("target_bookmark", target.bookmark);
            if let Some(version) = version {
                scuba.add("version", version);
            }
            scuba.add("method", method);
            scuba
        })
    }

    async fn call_and_log(
        &self,
        ctx: &CoreContext,
        target: &Target,
        version: Option<&SyncConfigVersion>,
        f: impl Future<Output = Result<ChangesetId, MegarepoError>>,
        method: &str,
    ) -> Result<ChangesetId, MegarepoError> {
        let ctx = self.prepare_ctx(ctx, target.clone(), version.cloned(), method);
        ctx.scuba().clone().log_with_msg("Started", None);
        let res = f.await;
        match &res {
            Ok(cs_id) => {
                ctx.scuba()
                    .clone()
                    .add("Result", format!("{}", cs_id))
                    .log_with_msg("Success", None);
            }
            Err(err) => {
                ctx.scuba()
                    .clone()
                    .log_with_msg("Failed", Some(format!("{:#?}", err)));
            }
        }
        res
    }

    /// Adds new sync target. Returs the commit hash of newly created target's head.
    pub async fn add_sync_target(
        &self,
        ctx: &CoreContext,
        sync_target_config: SyncTargetConfig,
        changesets_to_merge: HashMap<String, ChangesetId>,
        message: Option<String>,
    ) -> Result<ChangesetId, MegarepoError> {
        let mutable_renames = self
            .mutable_renames(ctx, &sync_target_config.target)
            .await?;
        let add_sync_target =
            AddSyncTarget::new(&self.megarepo_configs, &self.mononoke, &mutable_renames);

        let changesets_to_merge = changesets_to_merge
            .into_iter()
            .map(|(source, cs_id)| (SourceName(source), cs_id))
            .collect();

        let target = sync_target_config.target.clone();
        let version = sync_target_config.version.clone();
        let fut = add_sync_target.run(ctx, sync_target_config, changesets_to_merge, message);

        self.call_and_log(ctx, &target, Some(&version), fut, "add_sync_target")
            .await
    }

    pub async fn add_branching_sync_target(
        &self,
        ctx: &CoreContext,
        target: Target,
        branching_point: ChangesetId,
        source_target: Target,
    ) -> Result<ChangesetId, MegarepoError> {
        let add_branching_sync_target =
            AddBranchingSyncTarget::new(&self.megarepo_configs, &self.mononoke);
        let sync_target_config = add_branching_sync_target
            .fork_new_sync_target_config(ctx, target.clone(), branching_point, source_target)
            .await?;

        let version = sync_target_config.version.clone();
        let fut = add_branching_sync_target.run(ctx, sync_target_config, branching_point);
        self.call_and_log(
            ctx,
            &target,
            Some(&version),
            fut,
            "add_branching_sync_target",
        )
        .await
    }

    /// Syncs single changeset, returns the changeset it in the target.
    pub async fn sync_changeset(
        &self,
        ctx: &CoreContext,
        source_cs_id: ChangesetId,
        source_name: String,
        target: Target,
        target_location: ChangesetId,
    ) -> Result<ChangesetId, MegarepoError> {
        let mutable_renames = self.mutable_renames(ctx, &target).await?;
        let target_megarepo_mapping = self.megarepo_mapping(ctx, &target).await?;
        let source_name = SourceName::new(source_name);
        let sync_changeset = sync_changeset::SyncChangeset::new(
            &self.megarepo_configs,
            &self.mononoke,
            &target_megarepo_mapping,
            &mutable_renames,
        );
        let fut = sync_changeset.sync(ctx, source_cs_id, &source_name, &target, target_location);

        self.call_and_log(ctx, &target, None, fut, "sync_changeset")
            .await
    }

    /// Adds new sync target. Returs the commit hash of newly created target's head.
    pub async fn change_target_config(
        &self,
        ctx: &CoreContext,
        target: Target,
        new_version: SyncConfigVersion,
        target_location: ChangesetId,
        changesets_to_merge: HashMap<String, ChangesetId>,
        message: Option<String>,
    ) -> Result<ChangesetId, MegarepoError> {
        let mutable_renames = self.mutable_renames(ctx, &target).await?;
        let change_target_config =
            ChangeTargetConfig::new(&self.megarepo_configs, &self.mononoke, &mutable_renames);
        let changesets_to_merge = changesets_to_merge
            .into_iter()
            .map(|(source, cs_id)| (SourceName(source), cs_id))
            .collect();

        let version = new_version.clone();
        let fut = change_target_config.run(
            ctx,
            &target,
            new_version,
            target_location,
            changesets_to_merge,
            message,
        );

        self.call_and_log(ctx, &target, Some(&version), fut, "change_target_config")
            .await
    }

    pub async fn remerge_source(
        &self,
        ctx: &CoreContext,
        source_name: String,
        remerge_cs_id: ChangesetId,
        message: Option<String>,
        target: &Target,
        target_location: ChangesetId,
    ) -> Result<ChangesetId, MegarepoError> {
        let mutable_renames = self.mutable_renames(ctx, target).await?;
        let remerge_source =
            RemergeSource::new(&self.megarepo_configs, &self.mononoke, &mutable_renames);

        let source_name = SourceName(source_name);

        let fut = remerge_source.run(
            ctx,
            &source_name,
            remerge_cs_id,
            message,
            target,
            target_location,
        );

        self.call_and_log(ctx, target, None, fut, "remerge_source")
            .await
    }
}

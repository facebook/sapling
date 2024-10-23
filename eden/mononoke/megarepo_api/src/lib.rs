/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(async_closure)]
#![feature(trait_alias)]

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
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bookmarks::BookmarksRef;
use change_target_config::ChangeTargetConfig;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use context::CoreContext;
use filestore::FilestoreConfigRef;
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
use metaconfig_types::RepoConfigRef;
use mononoke_api::Mononoke;
use mononoke_api::MononokeRepo;
use mononoke_api::RepoContext;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mutable_renames::MutableRenames;
use mutable_renames::MutableRenamesRef;
use parking_lot::Mutex;
use remerge_source::RemergeSource;
use repo_authorization::AuthorizationContext;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_factory::RepoFactory;
use repo_identity::ArcRepoIdentity;
use repo_identity::RepoIdentityArc;
use repo_identity::RepoIdentityRef;
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

pub trait Repo = BonsaiHgMappingRef
    + BookmarksRef
    + CommitGraphRef
    + CommitGraphWriterRef
    + FilestoreConfigRef
    + MutableRenamesRef
    + RepoBlobstoreArc
    + RepoBlobstoreRef
    + RepoConfigArc
    + RepoConfigRef
    + RepoDerivedDataRef
    + RepoIdentityRef
    + Send
    + Sync;

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

pub struct MegarepoApi<R> {
    megarepo_configs: Arc<dyn MononokeMegarepoConfigs>,
    megarepo_mapping_cache: Cache<ArcRepoIdentity, Arc<MegarepoMapping>>,
    mononoke: Arc<Mononoke<R>>,
    repo_factory: Arc<RepoFactory>,
}

impl<R: MononokeRepo> MegarepoApi<R> {
    pub fn new(app: &MononokeApp, mononoke: Arc<Mononoke<R>>) -> Result<Self, MegarepoError> {
        let env = app.environment();
        let fb = env.fb;
        let logger = env.logger.new(o!("megarepo" => ""));

        let megarepo_configs: Arc<dyn MononokeMegarepoConfigs> = match &env.megarepo_configs_options
        {
            MononokeMegarepoConfigsOptions::Prod
            | MononokeMegarepoConfigsOptions::IntegrationTest(_) => {
                Arc::new(CfgrMononokeMegarepoConfigs::new(
                    fb,
                    &logger,
                    env.mysql_options.clone(),
                    env.readonly_storage,
                )?)
            }
            MononokeMegarepoConfigsOptions::UnitTest => {
                Arc::new(TestMononokeMegarepoConfigs::new(&logger))
            }
        };

        let repo_factory = app.repo_factory().clone();

        Ok(Self {
            megarepo_configs,
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
            target_repo.repo(),
            *cs_id,
            target,
            &self.megarepo_configs,
        )
        .await
    }

    /// Get mononoke object
    pub fn mononoke(&self) -> Arc<Mononoke<R>> {
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

    /// Get Mononoke repo context by target
    pub async fn target_repo(
        &self,
        ctx: &CoreContext,
        target: &Target,
    ) -> Result<RepoContext<R>, Error> {
        let repo_id: i32 = TryFrom::<i64>::try_from(target.repo_id)?;
        let repo_id = RepositoryId::new(repo_id);
        let repo = self
            .mononoke
            .repo_by_id(ctx.clone(), repo_id)
            .await
            .map_err(MegarepoError::internal)?
            .ok_or_else(|| {
                if repo_id.id() != 0 // special case for some tests
                    && justknobs::eval(
                        "scm/mononoke:megarepo_panic_on_repo_not_found_error",
                        None,
                        Some(&repo_id.to_string()),
                    )
                    .unwrap_or(false)
                {
                    panic!("repo not found {}", repo_id)
                } else {
                    MegarepoError::request(anyhow!("repo not found {}", repo_id))
                }
            })?
            .with_authorization_context(AuthorizationContext::new_bypass_access_control())
            .build()
            .await?;
        Ok(repo)
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
                    .open::<MegarepoMapping>()
                    .await?;

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

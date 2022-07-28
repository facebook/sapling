/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use cacheblob::LeaseOps;
use getbundle_response::SessionLfsParams;
use hooks::HookManager;
use live_commit_sync_config::LiveCommitSyncConfig;
use metaconfig_types::InfinitepushParams;
use metaconfig_types::PushParams;
use metaconfig_types::PushrebaseParams;
use mononoke_api::Repo;
use mononoke_api_types::InnerRepo;
use mononoke_types::RepositoryId;
use rand::Rng;
use reachabilityindex::LeastCommonAncestorsHint;
use repo_lock::RepoLock;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::fmt::Debug;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;
use streaming_clone::StreamingClone;
use streaming_clone::StreamingCloneRef;
use warm_bookmarks_cache::BookmarksCache;

#[derive(Clone)]
pub struct MononokeRepo {
    repo: Arc<Repo>,
}

impl MononokeRepo {
    pub async fn new(repo: Arc<Repo>) -> Result<Self, Error> {
        Ok(Self { repo })
    }

    pub fn blobrepo(&self) -> &BlobRepo {
        self.repo.blob_repo()
    }

    pub fn inner_repo(&self) -> &InnerRepo {
        self.repo.inner_repo()
    }

    pub fn pushrebase_params(&self) -> &PushrebaseParams {
        &self.repo.config().pushrebase
    }

    pub fn hipster_acl(&self) -> &Option<String> {
        &self.repo.config().hipster_acl
    }

    pub fn push_params(&self) -> &PushParams {
        &self.repo.config().push
    }

    pub fn repo_client_use_warm_bookmarks_cache(&self) -> bool {
        self.repo.config().repo_client_use_warm_bookmarks_cache
    }

    pub fn hook_manager(&self) -> Arc<HookManager> {
        self.repo.hook_manager().clone()
    }

    pub fn streaming_clone(&self) -> &StreamingClone {
        self.repo.inner_repo().streaming_clone()
    }

    pub fn force_lfs_if_threshold_set(&self) -> SessionLfsParams {
        SessionLfsParams {
            threshold: self.repo.config().lfs.threshold,
        }
    }

    pub fn lfs_params(&self, client_hostname: Option<&str>) -> SessionLfsParams {
        let percentage = self.repo.config().lfs.rollout_percentage;

        let allowed = match client_hostname {
            Some(client_hostname) => {
                let mut hasher = DefaultHasher::new();
                client_hostname.hash(&mut hasher);
                hasher.finish() % 100 < percentage.into()
            }
            None => {
                // Randomize in case source hostname is not set to avoid
                // sudden jumps in traffic
                rand::thread_rng().gen_ratio(percentage, 100)
            }
        };

        if allowed {
            SessionLfsParams {
                threshold: self.repo.config().lfs.threshold,
            }
        } else {
            SessionLfsParams { threshold: None }
        }
    }

    pub fn reponame(&self) -> &String {
        self.repo.name()
    }

    pub fn repoid(&self) -> RepositoryId {
        self.repo.repoid()
    }

    pub fn repo_lock(&self) -> Arc<dyn RepoLock> {
        self.repo.blob_repo().repo_lock()
    }

    pub fn infinitepush(&self) -> &InfinitepushParams {
        &self.repo.config().infinitepush
    }

    pub fn list_keys_patterns_max(&self) -> u64 {
        self.repo.config().list_keys_patterns_max
    }

    pub fn lca_hint(&self) -> Arc<dyn LeastCommonAncestorsHint> {
        self.repo.skiplist_index().clone()
    }

    pub fn warm_bookmarks_cache(&self) -> &Arc<dyn BookmarksCache> {
        self.repo.warm_bookmarks_cache()
    }

    pub fn live_commit_sync_config(&self) -> Arc<dyn LiveCommitSyncConfig> {
        self.repo.live_commit_sync_config()
    }

    pub fn x_repo_sync_lease(&self) -> &Arc<dyn LeaseOps> {
        self.repo.x_repo_sync_lease()
    }
}

impl Debug for MononokeRepo {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "MononokeRepo({:#?})", self.repo.repoid())
    }
}

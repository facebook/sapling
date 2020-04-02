/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[deny(warnings)]
use anyhow::Error;
use blobrepo::BlobRepo;
use cloned::cloned;
use fbinit::FacebookInit;
use futures::TryFutureExt;
use futures_ext::{BoxFuture, FutureExt};
use futures_old::future::Future;
use getbundle_response::SessionLfsParams;
use hooks::HookManager;
use metaconfig_types::{
    BookmarkAttrs, BookmarkParams, InfinitepushParams, LfsParams, PushrebaseParams, RepoReadOnly,
};
use mononoke_types::RepositoryId;
use mutable_counters::MutableCounters;
use rand::Rng;
use reachabilityindex::LeastCommonAncestorsHint;
use repo_blobstore::RepoBlobstore;
use repo_read_write_status::RepoReadWriteFetcher;
use slog::{error, Logger};
use smc::get_available_services;
use sql_construct::facebook::FbSqlConstruct;
use sql_ext::facebook::MysqlOptions;
use std::fmt::{self, Debug};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use std::{
    collections::{hash_map::DefaultHasher, HashSet},
    hash::{Hash, Hasher},
};
use streaming_clone::SqlStreamingChunksFetcher;
use tokio::task::spawn_blocking;

pub use builder::MononokeRepoBuilder;

mod builder;

#[derive(Clone)]
pub struct SqlStreamingCloneConfig {
    pub blobstore: RepoBlobstore,
    pub fetcher: SqlStreamingChunksFetcher,
    pub repoid: RepositoryId,
}

#[derive(Clone)]
pub struct MononokeRepo {
    blobrepo: BlobRepo,
    pushrebase_params: PushrebaseParams,
    hook_manager: Arc<HookManager>,
    streaming_clone: Option<SqlStreamingCloneConfig>,
    lfs_params: LfsParams,
    readonly_fetcher: RepoReadWriteFetcher,
    bookmark_attrs: BookmarkAttrs,
    infinitepush: InfinitepushParams,
    list_keys_patterns_max: u64,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    mutable_counters: Arc<dyn MutableCounters>,
    // Hostnames that always get lfs pointers.
    lfs_rolled_out_hostnames: Arc<RwLock<HashSet<String>>>,
}

impl MononokeRepo {
    #[inline]
    pub async fn new(
        fb: FacebookInit,
        logger: Logger,
        blobrepo: BlobRepo,
        pushrebase_params: &PushrebaseParams,
        bookmark_params: Vec<BookmarkParams>,
        hook_manager: Arc<HookManager>,
        streaming_clone: Option<SqlStreamingCloneConfig>,
        lfs_params: LfsParams,
        readonly_fetcher: RepoReadWriteFetcher,
        infinitepush: InfinitepushParams,
        list_keys_patterns_max: u64,
        lca_hint: Arc<dyn LeastCommonAncestorsHint>,
        mutable_counters: Arc<dyn MutableCounters>,
    ) -> Result<Self, Error> {
        let lfs_rolled_out_hostnames = Arc::new(RwLock::new(HashSet::new()));
        if let Some(rollout_smc_tier) = &lfs_params.rollout_smc_tier {
            spawn_smc_tier_fetcher(
                fb,
                &logger,
                lfs_rolled_out_hostnames.clone(),
                rollout_smc_tier.clone(),
            )
            .await;
        }

        Ok(MononokeRepo {
            blobrepo,
            pushrebase_params: pushrebase_params.clone(),
            hook_manager,
            streaming_clone,
            lfs_params,
            readonly_fetcher,
            bookmark_attrs: BookmarkAttrs::new(bookmark_params),
            infinitepush,
            list_keys_patterns_max,
            lca_hint,
            mutable_counters,
            lfs_rolled_out_hostnames,
        })
    }

    #[inline]
    pub fn blobrepo(&self) -> &BlobRepo {
        &self.blobrepo
    }

    pub fn pushrebase_params(&self) -> &PushrebaseParams {
        &self.pushrebase_params
    }

    pub fn bookmark_attrs(&self) -> BookmarkAttrs {
        self.bookmark_attrs.clone()
    }

    pub fn hook_manager(&self) -> Arc<HookManager> {
        self.hook_manager.clone()
    }

    pub fn streaming_clone(&self) -> &Option<SqlStreamingCloneConfig> {
        &self.streaming_clone
    }

    pub fn lfs_params(&self, client_hostname: Option<&str>) -> SessionLfsParams {
        let percentage = self.lfs_params.rollout_percentage;
        let allowed = match client_hostname {
            Some(client_hostname) => {
                let rolled_out_hostnames = self.lfs_rolled_out_hostnames.read().unwrap();
                if rolled_out_hostnames.contains(client_hostname) {
                    true
                } else {
                    let mut hasher = DefaultHasher::new();
                    client_hostname.hash(&mut hasher);
                    hasher.finish() % 100 < percentage.into()
                }
            }
            None => {
                // Randomize in case source hostname is not set to avoid
                // sudden jumps in traffic
                rand::thread_rng().gen_ratio(percentage.into(), 100)
            }
        };

        if allowed {
            SessionLfsParams {
                threshold: self.lfs_params.threshold,
            }
        } else {
            SessionLfsParams { threshold: None }
        }
    }

    pub fn reponame(&self) -> &String {
        &self.blobrepo.name()
    }

    #[inline]
    pub fn repoid(&self) -> RepositoryId {
        self.blobrepo.get_repoid()
    }

    pub fn readonly(&self) -> BoxFuture<RepoReadOnly, Error> {
        self.readonly_fetcher.readonly()
    }

    pub fn infinitepush(&self) -> &InfinitepushParams {
        &self.infinitepush
    }

    pub fn list_keys_patterns_max(&self) -> u64 {
        self.list_keys_patterns_max
    }

    pub fn lca_hint(&self) -> Arc<dyn LeastCommonAncestorsHint> {
        self.lca_hint.clone()
    }
}

async fn spawn_smc_tier_fetcher(
    fb: FacebookInit,
    logger: &Logger,
    lfs_rolled_out_hostnames: Arc<RwLock<HashSet<String>>>,
    rollout_smc_tier: String,
) {
    let update_rolled_out_hostnames = {
        move || {
            cloned!(lfs_rolled_out_hostnames, rollout_smc_tier);
            spawn_blocking(move || {
                let with_propagated = true;
                let services = get_available_services(fb, &rollout_smc_tier, with_propagated);
                if let Ok(services) = services {
                    let new_hostnames = services
                        .iter()
                        .filter(|service| service.is_enabled())
                        .map(|service| service.hostname.into_owned())
                        .collect();
                    {
                        let mut lfs_rolled_out_hostnames =
                            lfs_rolled_out_hostnames.write().unwrap();
                        *lfs_rolled_out_hostnames = new_hostnames;
                    }
                }
            })
        }
    };

    // Make sure initial update doesn't block service startup
    match tokio::time::timeout(Duration::from_secs(10), update_rolled_out_hostnames()).await {
        Ok(Ok(())) => {}
        Err(_elapsed) => {
            error!(
                logger,
                "timeout while waiting for initial fetch of smc tier"
            );
        }
        Ok(Err(err)) => {
            error!(logger, "failed to do initial fetch of smc tier, {:?}", err);
        }
    }

    tokio::spawn(async move {
        loop {
            let _ = update_rolled_out_hostnames().await;
            let delay = 30;
            let splay = rand::random::<u64>() % delay;
            tokio::time::delay_for(Duration::from_secs(delay + splay)).await;
        }
    });
}

pub fn streaming_clone(
    fb: FacebookInit,
    blobrepo: BlobRepo,
    db_address: String,
    mysql_options: MysqlOptions,
    repoid: RepositoryId,
    readonly_storage: bool,
) -> BoxFuture<SqlStreamingCloneConfig, Error> {
    SqlStreamingChunksFetcher::with_xdb(fb, db_address, mysql_options, readonly_storage)
        .compat()
        .map(move |fetcher| SqlStreamingCloneConfig {
            fetcher,
            blobstore: blobrepo.get_blobstore(),
            repoid,
        })
        .boxify()
}

impl Debug for MononokeRepo {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "MononokeRepo({:#?})", self.blobrepo.get_repoid())
    }
}

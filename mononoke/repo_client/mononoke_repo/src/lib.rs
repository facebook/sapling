/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#[deny(warnings)]
use anyhow::Error;
use blobrepo::BlobRepo;
use fbinit::FacebookInit;
use futures::future::Future;
use futures_ext::{BoxFuture, FutureExt};
use hooks::HookManager;
use metaconfig_types::{
    BookmarkAttrs, BookmarkParams, InfinitepushParams, LfsParams, PushrebaseParams, RepoReadOnly,
};
use mononoke_types::RepositoryId;
use mutable_counters::MutableCounters;
use phases::Phases;
use reachabilityindex::LeastCommonAncestorsHint;
use repo_blobstore::RepoBlobstore;
use repo_read_write_status::RepoReadWriteFetcher;
use sql_ext::MysqlOptions;
use sql_ext::SqlConstructors;
use std::fmt::{self, Debug};
use std::sync::Arc;
use streaming_clone::SqlStreamingChunksFetcher;

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
    reponame: String,
    readonly_fetcher: RepoReadWriteFetcher,
    bookmark_attrs: BookmarkAttrs,
    infinitepush: InfinitepushParams,
    list_keys_patterns_max: u64,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    phases_hint: Arc<dyn Phases>,
    mutable_counters: Arc<dyn MutableCounters>,
}

impl MononokeRepo {
    #[inline]
    pub fn new(
        blobrepo: BlobRepo,
        pushrebase_params: &PushrebaseParams,
        bookmark_params: Vec<BookmarkParams>,
        hook_manager: Arc<HookManager>,
        streaming_clone: Option<SqlStreamingCloneConfig>,
        lfs_params: LfsParams,
        reponame: String,
        readonly_fetcher: RepoReadWriteFetcher,
        infinitepush: InfinitepushParams,
        list_keys_patterns_max: u64,
        lca_hint: Arc<dyn LeastCommonAncestorsHint>,
        phases_hint: Arc<dyn Phases>,
        mutable_counters: Arc<dyn MutableCounters>,
    ) -> Self {
        MononokeRepo {
            blobrepo,
            pushrebase_params: pushrebase_params.clone(),
            hook_manager,
            streaming_clone,
            lfs_params,
            reponame,
            readonly_fetcher,
            bookmark_attrs: BookmarkAttrs::new(bookmark_params),
            infinitepush,
            list_keys_patterns_max,
            lca_hint,
            phases_hint,
            mutable_counters,
        }
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

    pub fn lfs_params(&self) -> &LfsParams {
        &self.lfs_params
    }

    pub fn reponame(&self) -> &String {
        &self.reponame
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

    pub fn phases_hint(&self) -> Arc<dyn Phases> {
        self.phases_hint.clone()
    }
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

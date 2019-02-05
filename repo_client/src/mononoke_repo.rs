// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use blobstore::{Blobstore, PrefixBlobstore};
use errors::*;
use hooks::HookManager;
use metaconfig_types::RepoReadOnly;
use metaconfig_types::{LfsParams, PushrebaseParams};
use mononoke_types::RepositoryId;
use std::fmt::{self, Debug};
use std::sync::Arc;
use streaming_clone::SqlStreamingChunksFetcher;

#[derive(Clone)]
pub struct SqlStreamingCloneConfig {
    pub blobstore: PrefixBlobstore<Arc<Blobstore>>,
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
    readonly: RepoReadOnly,
}

impl MononokeRepo {
    #[inline]
    pub fn new(
        blobrepo: BlobRepo,
        pushrebase_params: &PushrebaseParams,
        hook_manager: Arc<HookManager>,
        streaming_clone: Option<SqlStreamingCloneConfig>,
        lfs_params: LfsParams,
        reponame: String,
        readonly: RepoReadOnly,
    ) -> Self {
        MononokeRepo {
            blobrepo,
            pushrebase_params: pushrebase_params.clone(),
            hook_manager,
            streaming_clone,
            lfs_params,
            reponame,
            readonly,
        }
    }

    #[inline]
    pub fn blobrepo(&self) -> &BlobRepo {
        &self.blobrepo
    }

    pub fn pushrebase_params(&self) -> &PushrebaseParams {
        &self.pushrebase_params
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

    pub fn readonly(&self) -> RepoReadOnly {
        self.readonly
    }
}

pub fn streaming_clone(
    blobrepo: BlobRepo,
    db_address: &str,
    myrouter_port: u16,
    repoid: RepositoryId,
) -> Result<SqlStreamingCloneConfig> {
    let fetcher = SqlStreamingChunksFetcher::with_myrouter(db_address, myrouter_port);
    let streaming_clone = SqlStreamingCloneConfig {
        fetcher,
        blobstore: blobrepo.get_blobstore(),
        repoid,
    };

    Ok(streaming_clone)
}

impl Debug for MononokeRepo {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "MononokeRepo({:#?})", self.blobrepo.get_repoid())
    }
}

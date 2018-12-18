// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt::{self, Debug};
use std::sync::Arc;
use std::time::Duration;

use failure::err_msg;
use rand::distributions::{Distribution, LogNormal};
use rand::prelude::*;
use rand_hc::Hc128Rng;
use slog::Logger;

use scribe_cxx::ScribeCxxClient;

use blobrepo::BlobRepo;
use blobstore::{Blobstore, PrefixBlobstore};
use hooks::HookManager;
use mercurial_types::RepositoryId;
use metaconfig::{LfsParams, PushrebaseParams};
use metaconfig::repoconfig::{RepoReadOnly, RepoType};

use errors::*;

use client::streaming_clone::SqlStreamingChunksFetcher;

struct LogNormalGenerator {
    rng: Hc128Rng,
    distribution: LogNormal,
}

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

pub fn open_blobrepo(
    logger: Logger,
    repotype: RepoType,
    repoid: RepositoryId,
    myrouter_port: Option<u16>,
) -> Result<BlobRepo> {
    use metaconfig::repoconfig::RepoType::*;

    let blobrepo = match repotype {
        BlobFiles(ref path) => BlobRepo::new_files(logger, &path, repoid)?,
        BlobRocks(ref path) => BlobRepo::new_rocksdb(logger, &path, repoid)?,
        BlobRemote {
            ref blobstores_args,
            ref db_address,
            ref filenode_shards,
        } => BlobRepo::new_remote_scribe_commits(
            logger,
            blobstores_args,
            db_address.clone(),
            filenode_shards.clone(),
            repoid,
            myrouter_port.ok_or(err_msg(
                "Missing myrouter port, unable to open BlobRemote repo",
            ))?,
            ScribeCxxClient::new(),
        )?,
        TestBlobDelayRocks(ref path, mean, stddev) => {
            // We take in an arithmetic mean and stddev, and deduce a log normal
            let mean = mean as f64 / 1_000_000.0;
            let stddev = stddev as f64 / 1_000_000.0;
            let variance = stddev * stddev;
            let mean_squared = mean * mean;

            let mu = (mean_squared / (variance + mean_squared).sqrt()).ln();
            let sigma = (1.0 + variance / mean_squared).ln();

            let max_delay = 16.0;

            let mut delay_gen = LogNormalGenerator {
                // This is a deterministic RNG if not seeded
                rng: Hc128Rng::from_seed([0; 32]),
                distribution: LogNormal::new(mu, sigma),
            };
            let delay_gen = move |()| {
                let delay = delay_gen.distribution.sample(&mut delay_gen.rng);
                let delay = if delay < 0.0 || delay > max_delay {
                    max_delay
                } else {
                    delay
                };
                let seconds = delay as u64;
                let nanos = ((delay - seconds as f64) * 1_000_000_000.0) as u32;
                Duration::new(seconds, nanos)
            };
            BlobRepo::new_rocksdb_delayed(
                logger,
                &path,
                repoid,
                delay_gen,
                // Roundtrips to the server - i.e. how many delays to apply
                2, // get
                3, // put
                2, // is_present
                2, // assert_present
            )?
        }
    };

    Ok(blobrepo)
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

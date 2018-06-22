// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;

use actix::{Actor, Addr, Context, Handler, Message, Syn};
use actix::dev::Request;
use failure::{err_msg, Error, Result};
use futures::{Future, IntoFuture};
use slog::Logger;

use blobrepo::BlobRepo;
use mercurial_types::RepositoryId;
use metaconfig::repoconfig::{RepoConfig, RepoConfigs};
use metaconfig::repoconfig::RepoType::{BlobRocks, TestBlobManifold};

#[derive(Debug)]
pub enum MononokeRepoQuery {
    GetBlobContent { hash: String },
}

impl Message for MononokeRepoQuery {
    type Result = Result<String>;
}

pub struct MononokeQuery {
    pub kind: MononokeRepoQuery,
    pub repo: String,
}

impl Message for MononokeQuery {
    type Result = Result<Request<Syn, MononokeRepoActor, MononokeRepoQuery>>;
}

pub struct MononokeRepoActor {
    pub repo: BlobRepo,
}

impl MononokeRepoActor {
    fn new(logger: Logger, config: RepoConfig) -> Result<Self> {
        let repoid = RepositoryId::new(config.repoid);
        let repo = match config.repotype {
            BlobRocks(ref path) => BlobRepo::new_rocksdb(logger, &path, repoid),
            TestBlobManifold {
                ref manifold_bucket,
                ref prefix,
                ref db_address,
                blobstore_cache_size,
                changesets_cache_size,
                filenodes_cache_size,
                io_thread_num,
                max_concurrent_requests_per_io_thread,
                ..
            } => BlobRepo::new_test_manifold(
                logger,
                manifold_bucket,
                &prefix,
                repoid,
                &db_address,
                blobstore_cache_size,
                changesets_cache_size,
                filenodes_cache_size,
                io_thread_num,
                max_concurrent_requests_per_io_thread,
            ),
            _ => Err(err_msg("Unsupported repo type.")),
        };

        repo.map(|repo| Self { repo })
    }
}

impl Actor for MononokeRepoActor {
    type Context = Context<Self>;
}

impl Handler<MononokeRepoQuery> for MononokeRepoActor {
    type Result = Result<String>;

    fn handle(&mut self, msg: MononokeRepoQuery, _ctx: &mut Context<Self>) -> Self::Result {
        use MononokeRepoQuery::*;

        match msg {
            GetBlobContent { hash: _hash } => Ok("success!".to_string()),
        }
    }
}

pub struct MononokeActor {
    repos: HashMap<String, Addr<Syn, MononokeRepoActor>>,
}

impl MononokeActor {
    pub fn new(logger: Logger, config: RepoConfigs) -> Self {
        let logger = logger.clone();
        let repos = config
            .repos
            .into_iter()
            .map(move |(reponame, config)| {
                let logger = logger.clone();
                (
                    reponame,
                    MononokeRepoActor::create(move |_| {
                        MononokeRepoActor::new(logger, config).expect("Unable to initialize repo")
                    }),
                )
            })
            .collect();

        Self { repos }
    }
}

impl Actor for MononokeActor {
    type Context = Context<Self>;
}

impl Handler<MononokeQuery> for MononokeActor {
    type Result = Result<Request<Syn, MononokeRepoActor, MononokeRepoQuery>>;

    fn handle(&mut self, msg: MononokeQuery, _ctx: &mut Context<Self>) -> Self::Result {
        match self.repos.get(&msg.repo) {
            Some(repo) => Ok(repo.send(msg.kind)),
            None => Err(err_msg("repo not found!")),
        }
    }
}

pub fn unwrap_request(
    request: Request<Syn, MononokeActor, MononokeQuery>,
) -> impl Future<Item = String, Error = Error> {
    request.map_err(From::from).and_then(|result| {
        result.map_err(From::from).into_future().and_then(|result| {
            result
                .map_err(From::from)
                .and_then(|result| result.map_err(From::from).into_future())
        })
    })
}

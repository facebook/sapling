// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::str::FromStr;

use actix::{Actor, Addr, Context, Handler, Message, Syn};
use actix::dev::Request;
use bytes::Bytes;
use failure::{err_msg, Error, FutureFailureErrorExt, Result, ResultExt};
use futures::{Future, IntoFuture};
use futures_ext::BoxFuture;
use slog::Logger;

use blobrepo::BlobRepo;
use futures_ext::FutureExt;
use mercurial_types::{HgNodeHash, RepositoryId};
use metaconfig::repoconfig::{RepoConfig, RepoConfigs};
use metaconfig::repoconfig::RepoType::{BlobManifold, BlobRocks};

use errors::ErrorKind;

#[derive(Debug)]
pub enum MononokeRepoQuery {
    GetBlobContent { hash: String },
}

impl Message for MononokeRepoQuery {
    type Result = Result<BoxFuture<MononokeRepoResponse, Error>>;
}

pub enum MononokeRepoResponse {
    GetBlobContent { content: Bytes },
}

pub struct MononokeQuery {
    pub kind: MononokeRepoQuery,
    pub repo: String,
}

impl Message for MononokeQuery {
    type Result = Result<Request<Syn, MononokeRepoActor, MononokeRepoQuery>>;
}

pub struct MononokeRepoActor {
    repo: BlobRepo,
}

impl MononokeRepoActor {
    fn new(logger: Logger, config: RepoConfig) -> Result<Self> {
        let repoid = RepositoryId::new(config.repoid);
        let repo = match config.repotype {
            BlobRocks(ref path) => BlobRepo::new_rocksdb(logger, &path, repoid),
            BlobManifold {
                ref manifold_bucket,
                ref prefix,
                ref db_address,
                blobstore_cache_size,
                changesets_cache_size,
                filenodes_cache_size,
                io_thread_num,
                max_concurrent_requests_per_io_thread,
                ..
            } => BlobRepo::new_manifold(
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
    type Result = Result<BoxFuture<MononokeRepoResponse, Error>>;

    fn handle(&mut self, msg: MononokeRepoQuery, _ctx: &mut Context<Self>) -> Self::Result {
        use MononokeRepoQuery::*;

        match msg {
            GetBlobContent { hash } => HgNodeHash::from_str(&hash)
                .with_context(|_| ErrorKind::InvalidInput(hash.clone()))
                .map_err(From::from)
                .map(|node_hash| {
                    self.repo
                        .get_file_content(&node_hash)
                        .map(|content| MononokeRepoResponse::GetBlobContent {
                            content: content.into_bytes(),
                        })
                        .with_context(move |_| ErrorKind::NotFound(hash.clone()))
                        .from_err()
                        .boxify()
                }),
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

    fn handle(
        &mut self,
        MononokeQuery { repo, kind, .. }: MononokeQuery,
        _ctx: &mut Context<Self>,
    ) -> Self::Result {
        match self.repos.get(&repo) {
            Some(repo) => Ok(repo.send(kind)),
            None => Err(ErrorKind::NotFound(repo).into()),
        }
    }
}

pub fn unwrap_request(
    request: Request<Syn, MononokeActor, MononokeQuery>,
) -> impl Future<Item = MononokeRepoResponse, Error = Error> {
    request
        .into_future()
        .from_err()
        .and_then(|result| result)
        .and_then(|result| result.map_err(From::from))
        .and_then(|result| result)
        .and_then(|result| result)
}

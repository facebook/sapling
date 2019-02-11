// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

mod lfs;
mod model;
mod query;
mod repo;
mod response;

use std::collections::HashMap;

use context::CoreContext;
use failure::Error;
use futures::{future::join_all, Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use tracing::TraceContext;
use uuid::Uuid;

use metaconfig_parser::RepoConfigs;

use errors::ErrorKind;

pub use self::lfs::BatchRequest;
pub use self::query::{MononokeQuery, MononokeRepoQuery, Revision};
pub use self::repo::MononokeRepo;
pub use self::response::MononokeRepoResponse;

pub struct Mononoke {
    repos: HashMap<String, MononokeRepo>,
    logger: Logger,
    scuba_builder: ScubaSampleBuilder,
}

impl Mononoke {
    pub fn new(
        logger: Logger,
        config: RepoConfigs,
        myrouter_port: Option<u16>,
        scuba_builder: ScubaSampleBuilder,
        with_skiplist: bool,
    ) -> impl Future<Item = Self, Error = Error> {
        let log = logger.clone();
        join_all(
            config
                .repos
                .into_iter()
                .filter(move |&(_, ref config)| config.enabled)
                .map({
                    move |(name, config)| {
                        MononokeRepo::new(log.clone(), config, myrouter_port, with_skiplist)
                            .map(|repo| (name, repo))
                    }
                }),
        )
        .map(move |repos| Self {
            repos: repos.into_iter().collect(),
            logger: logger.clone(),
            scuba_builder,
        })
    }

    pub fn send_query(
        &self,
        MononokeQuery { repo, kind, .. }: MononokeQuery,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let session_uuid = Uuid::new_v4();

        let ctx = CoreContext::new(
            session_uuid,
            self.logger.clone(),
            self.scuba_builder.clone(),
            None,
            TraceContext::default(),
        );

        match self.repos.get(&repo) {
            Some(repo) => repo.send_query(ctx, kind),
            None => match kind {
                MononokeRepoQuery::LfsBatch { .. } => {
                    // LFS batch request require error in the different format:
                    // json: {"message": "Error message here"}
                    Err(ErrorKind::LFSNotFound(repo)).into_future().boxify()
                }
                _ => Err(ErrorKind::NotFound(repo, None)).into_future().boxify(),
            },
        }
    }
}

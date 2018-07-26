// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

mod query;
mod repo;
mod response;

use std::collections::HashMap;

use actix::{Actor, Addr, Context, Handler};
use actix::dev::Request;
use failure::Error;
use futures::{Future, IntoFuture};
use slog::Logger;

use metaconfig::repoconfig::RepoConfigs;

use errors::ErrorKind;

pub use self::query::{MononokeQuery, MononokeRepoQuery};
pub use self::repo::MononokeRepoActor;
pub use self::response::MononokeRepoResponse;

pub struct MononokeActor {
    repos: HashMap<String, Addr<MononokeRepoActor>>,
}

impl MononokeActor {
    pub fn new(logger: Logger, config: RepoConfigs) -> Self {
        let logger = logger.clone();
        let repos = config
            .repos
            .into_iter()
            .filter(move |&(_, ref config)| config.enabled)
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
    type Result = Result<Request<MononokeRepoActor, MononokeRepoQuery>, Error>;

    fn handle(
        &mut self,
        MononokeQuery { repo, kind, .. }: MononokeQuery,
        _ctx: &mut Context<Self>,
    ) -> Self::Result {
        match self.repos.get(&repo) {
            Some(repo) => Ok(repo.send(kind)),
            None => Err(ErrorKind::NotFound(repo, None).into()),
        }
    }
}

pub fn unwrap_request(
    request: Request<MononokeActor, MononokeQuery>,
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    request
        .into_future()
        .from_err()
        .and_then(|result| result)  // use flatten here will blind the compiler.
        .and_then(|result| result.map_err(From::from))
        .flatten()
        .flatten()
        .from_err()
}

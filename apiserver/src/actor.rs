// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::convert::TryFrom;
use std::result::Result as StdResult;
use std::str::FromStr;
use std::sync::Arc;

use actix::{Actor, Addr, Context, Handler, Message, Syn};
use actix::dev::Request;
use actix_web::{Body, HttpRequest, HttpResponse, Responder};
use bytes::Bytes;
use failure::{err_msg, Error, FutureFailureErrorExt, ResultExt};
use futures::{Future, IntoFuture};
use futures_ext::BoxFuture;
use slog::Logger;

use blobrepo::BlobRepo;
use futures_ext::FutureExt;
use mercurial_types::{Changeset, HgChangesetId, RepositoryId};
use mercurial_types::manifest::Content;
use metaconfig::repoconfig::{RepoConfig, RepoConfigs};
use metaconfig::repoconfig::RepoType::{BlobManifold, BlobRocks};
use mononoke_types::MPath;

use errors::ErrorKind;

#[derive(Debug)]
pub enum MononokeRepoQuery {
    GetRawFile { path: String, changeset: String },
}

impl Message for MononokeRepoQuery {
    type Result = Result<BoxFuture<MononokeRepoResponse, Error>, Error>;
}

pub enum MononokeRepoResponse {
    GetRawFile { content: Bytes },
}

fn binary_response(content: Bytes) -> HttpResponse {
    HttpResponse::Ok()
        .content_type("application/octet-stream")
        .body(Body::Binary(content.into()))
}

impl Responder for MononokeRepoResponse {
    type Item = HttpResponse;
    type Error = ErrorKind;

    fn respond_to<S: 'static>(self, _req: &HttpRequest<S>) -> StdResult<Self::Item, Self::Error> {
        use MononokeRepoResponse::*;

        match self {
            GetRawFile { content } => Ok(binary_response(content)),
        }
    }
}

pub struct MononokeQuery {
    pub kind: MononokeRepoQuery,
    pub repo: String,
}

impl Message for MononokeQuery {
    type Result = Result<Request<Syn, MononokeRepoActor, MononokeRepoQuery>, Error>;
}

pub struct MononokeRepoActor {
    repo: Arc<BlobRepo>,
    logger: Logger,
}

impl MononokeRepoActor {
    fn new(logger: Logger, config: RepoConfig) -> Result<Self, Error> {
        let repoid = RepositoryId::new(config.repoid);
        let repo = match config.repotype {
            BlobRocks(ref path) => BlobRepo::new_rocksdb(logger.clone(), &path, repoid),
            BlobManifold { ref args, .. } => BlobRepo::new_manifold(logger.clone(), args, repoid),
            _ => Err(err_msg("Unsupported repo type.")),
        };

        repo.map(|repo| Self {
            repo: Arc::new(repo),
            logger: logger,
        })
    }

    fn get_raw_file(
        &self,
        changeset: String,
        path: String,
    ) -> Result<BoxFuture<MononokeRepoResponse, Error>, Error> {
        // steps to get blob content by path
        // 1. Convert manifest to HgNodeHash
        // 2. Convert path to RepoPath / MPath
        // 3. Lookup NodeHashId in Manifest using path
        // 4. Unwrap FileContents
        // 5. Send!
        debug!(
            self.logger,
            "Retrieving file content of {} at changeset {}.", path, changeset
        );
        let mpath = MPath::try_from(&*path)
            .with_context(|_| ErrorKind::InvalidInput("path".into()))
            .map_err(|e| -> Error { e.into() })?;

        let changesetid = HgChangesetId::from_str(&changeset)
            .with_context(|_| ErrorKind::InvalidInput(changeset))
            .map_err(|e| -> Error { e.into() })?;

        let repo = self.repo.clone();

        Ok(repo.get_changeset_by_changesetid(&changesetid)
            .with_context(move |_| ErrorKind::NotFound(changesetid.to_string()))
            .from_err()
            .map(|changeset| changeset.manifestid().clone().into_nodehash())
            .and_then(move |manifestid| repo.find_path_in_manifest(Some(mpath), manifestid))
            .from_err()
            .and_then({
                let path = path.clone();
                |content| content.ok_or_else(move || ErrorKind::NotFound(path.to_string()))
            })
            .and_then(move |content| match content {
                Content::File(content)
                | Content::Executable(content)
                | Content::Symlink(content) => Ok(MononokeRepoResponse::GetRawFile {
                    content: content.into_bytes(),
                }),
                _ => Err(ErrorKind::InvalidInput(path.to_string())),
            })
            .from_err()
            .boxify())
    }
}

impl Actor for MononokeRepoActor {
    type Context = Context<Self>;
}

impl Handler<MononokeRepoQuery> for MononokeRepoActor {
    type Result = Result<BoxFuture<MononokeRepoResponse, Error>, Error>;

    fn handle(&mut self, msg: MononokeRepoQuery, _ctx: &mut Context<Self>) -> Self::Result {
        use MononokeRepoQuery::*;

        match msg {
            GetRawFile { changeset, path } => self.get_raw_file(changeset, path),
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
    type Result = Result<Request<Syn, MononokeRepoActor, MononokeRepoQuery>, Error>;

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
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    request
        .into_future()
        .from_err()
        .and_then(|result| result)
        .and_then(|result| result.map_err(From::from))
        .and_then(|result| result)
        .and_then(|result| result)
        .from_err()
}

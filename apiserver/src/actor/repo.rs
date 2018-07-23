// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::str::FromStr;
use std::sync::Arc;

use actix::{Actor, Context, Handler};
use failure::{err_msg, Error, Result, ResultExt};
use futures::Future;
use futures_ext::BoxFuture;
use slog::Logger;

use api;
use blobrepo::BlobRepo;
use futures_ext::FutureExt;
use mercurial_types::{HgNodeHash, RepositoryId};
use mercurial_types::manifest::Content;
use metaconfig::repoconfig::RepoConfig;
use metaconfig::repoconfig::RepoType::{BlobManifold, BlobRocks};
use reachabilityindex::{GenerationNumberBFS, ReachabilityIndex};

use errors::ErrorKind;
use from_string as FS;

use super::{MononokeRepoQuery, MononokeRepoResponse};

pub struct MononokeRepoActor {
    repo: Arc<BlobRepo>,
    logger: Logger,
}

impl MononokeRepoActor {
    pub fn new(logger: Logger, config: RepoConfig) -> Result<Self> {
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
    ) -> Result<BoxFuture<MononokeRepoResponse, Error>> {
        debug!(
            self.logger,
            "Retrieving file content of {} at changeset {}.", path, changeset
        );

        let mpath = FS::get_mpath(path.clone())?;
        let changesetid = FS::get_changeset_id(changeset)?;
        let repo = self.repo.clone();

        Ok(api::get_content_by_path(repo, changesetid, mpath)
            .and_then(move |content| match content {
                Content::File(content)
                | Content::Executable(content)
                | Content::Symlink(content) => Ok(MononokeRepoResponse::GetRawFile {
                    content: content.into_bytes(),
                }),
                _ => Err(ErrorKind::InvalidInput(path.to_string()).into()),
            })
            .from_err()
            .boxify())
    }

    fn is_ancestor(
        &self,
        proposed_ancestor: String,
        proposed_descendent: String,
    ) -> Result<BoxFuture<MononokeRepoResponse, Error>> {
        let mut genbfs = GenerationNumberBFS::new();
        let src_hash = HgNodeHash::from_str(&proposed_descendent)
            .with_context(|_| ErrorKind::InvalidInput(proposed_descendent))
            .map_err(|e| -> Error { e.into() })?;
        let dst_hash = HgNodeHash::from_str(&proposed_ancestor)
            .with_context(|_| ErrorKind::InvalidInput(proposed_ancestor))
            .map_err(|e| -> Error { e.into() })?;
        Ok(genbfs
            .query_reachability(self.repo.clone(), src_hash, dst_hash)
            .map(move |answer| MononokeRepoResponse::IsAncestor { answer: answer })
            .from_err()
            .boxify())
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
            GetRawFile { changeset, path } => self.get_raw_file(changeset, path),
            IsAncestor {
                proposed_ancestor,
                proposed_descendent,
            } => self.is_ancestor(proposed_ancestor, proposed_descendent),
        }
    }
}

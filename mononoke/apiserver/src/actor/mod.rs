/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::HashMap;

use anyhow::Error;
use blobrepo_factory::{BlobstoreOptions, Caching, ReadOnlyStorage};
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{
    future::{join_all, lazy},
    Future, IntoFuture,
};
use futures_ext::{BoxFuture, FutureExt};
use slog::{debug, info, o, Logger};
use sql_ext::MysqlOptions;

use metaconfig_parser::RepoConfigs;

use crate::cache::CacheManager;
use crate::errors::ErrorKind;

mod file_stream;
mod model;
mod query;
mod repo;
mod response;

pub use self::query::{MononokeQuery, MononokeRepoQuery, Revision};
pub use self::repo::MononokeRepo;
pub use self::response::MononokeRepoResponse;

pub struct Mononoke {
    pub(crate) repos: HashMap<String, MononokeRepo>,
}

impl Mononoke {
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        configs: RepoConfigs,
        mysql_options: MysqlOptions,
        readonly_storage: ReadOnlyStorage,
        blobstore_options: BlobstoreOptions,
        cache: Option<CacheManager>,
        with_cachelib: Caching,
        with_skiplist: bool,
    ) -> impl Future<Item = Self, Error = Error> {
        let common_config = configs.common;
        join_all(
            configs
                .repos
                .into_iter()
                .filter(move |&(_, ref config)| config.enabled)
                .map({
                    move |(name, config)| {
                        cloned!(logger, blobstore_options, cache);
                        lazy({
                            cloned!(common_config);
                            move || {
                                info!(logger, "Initializing repo: {}", &name);
                                MononokeRepo::new(
                                    fb,
                                    logger.new(o!("repo" => name.clone())),
                                    config,
                                    common_config,
                                    mysql_options,
                                    readonly_storage,
                                    blobstore_options,
                                    cache,
                                    with_cachelib,
                                    with_skiplist,
                                )
                                .map(move |repo| {
                                    debug!(logger, "Initialized {}", &name);
                                    (name, repo)
                                })
                            }
                        })
                    }
                }),
        )
        .map(move |repos| Self {
            repos: repos.into_iter().collect(),
        })
    }

    pub fn send_query(
        &self,
        ctx: CoreContext,
        MononokeQuery { repo, kind, .. }: MononokeQuery,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        match self.repos.get(&repo) {
            Some(repo) => repo.send_query(ctx, kind),
            None => Err(ErrorKind::NotFound(repo, None)).into_future().boxify(),
        }
    }
}

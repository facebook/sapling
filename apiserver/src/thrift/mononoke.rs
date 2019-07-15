// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{convert::TryInto, mem::size_of, sync::Arc};

use crate::errors::ErrorKind;
use apiserver_thrift::server::MononokeApiservice;
use apiserver_thrift::services::mononoke_apiservice::{
    GetBlobExn, GetBranchesExn, GetChangesetExn, GetRawExn, GetTreeExn, IsAncestorExn,
    ListDirectoryExn,
};
use apiserver_thrift::types::{
    MononokeBlob, MononokeBranches, MononokeChangeset, MononokeDirectory, MononokeGetBlobParams,
    MononokeGetBranchesParams, MononokeGetChangesetParams, MononokeGetRawParams,
    MononokeGetTreeParams, MononokeIsAncestorParams, MononokeListDirectoryParams, MononokeRevision,
};
use apiserver_thrift::MononokeRevision::UnknownField;
use cloned::cloned;
use context::CoreContext;
use failure::err_msg;
use futures::{Future, IntoFuture};
use futures_ext::BoxFuture;
use futures_stats::{FutureStats, Timed};
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use serde::Serialize;
use slog::Logger;
use time_ext::DurationExt;

use super::super::actor::{Mononoke, MononokeRepoResponse};

#[derive(Clone)]
pub struct MononokeAPIServiceImpl {
    addr: Arc<Mononoke>,
    logger: Logger,
    scuba_builder: ScubaSampleBuilder,
}

impl MononokeAPIServiceImpl {
    pub fn new(addr: Arc<Mononoke>, logger: Logger, scuba_builder: ScubaSampleBuilder) -> Self {
        Self {
            addr,
            logger,
            scuba_builder,
        }
    }

    fn create_scuba_logger<K: Serialize>(
        &self,
        method: &str,
        params_json: &K,
        path: Option<Vec<u8>>,
        revision: Option<MononokeRevision>,
    ) -> ScubaSampleBuilder {
        let mut scuba = self.scuba_builder.clone();
        scuba
            .add_common_server_data()
            .add("type", "thrift")
            .add("method", method)
            .add(
                "params",
                serde_json::to_string(params_json)
                    .unwrap_or_else(|_| "Error converting request to json".to_string()),
            );

        if let Some(path) = path {
            scuba.add(
                "path",
                String::from_utf8(path).unwrap_or("Invalid UTF-8 in path".to_string()),
            );
        }

        if let Some(rev) = revision {
            let rev = match rev {
                MononokeRevision::commit_hash(hash) => hash,
                MononokeRevision::bookmark(bookmark) => bookmark,
                UnknownField(_) => "Not a valid MononokeRevision".to_string(),
            };

            scuba.add("revision", rev);
        }

        scuba
    }

    fn create_ctx(&self) -> CoreContext {
        CoreContext::new_with_logger(self.logger.clone())
    }
}

fn log_time<T, U>(
    scuba: &mut ScubaSampleBuilder,
    stats: &FutureStats,
    resp: Result<T, U>,
    response_size: usize,
) {
    scuba
        .add_future_stats(&stats)
        .add("response_time", stats.completion_time.as_micros_unchecked())
        .add("response_size", response_size)
        .add(
            "success",
            match resp {
                Ok(_) => 1,
                Err(_) => 0,
            },
        );

    scuba.log();
}

impl MononokeApiservice for MononokeAPIServiceImpl {
    fn get_raw(&self, params: MononokeGetRawParams) -> BoxFuture<Vec<u8>, GetRawExn> {
        let ctx = self.create_ctx();

        let mut scuba = self.create_scuba_logger(
            "get_raw",
            &params,
            Some(params.path.clone()),
            Some(params.revision.clone()),
        );

        params
            .try_into()
            .into_future()
            .from_err()
            .and_then({
                cloned!(self.addr);
                move |param| addr.send_query(ctx, param)
            })
            .and_then(|resp: MononokeRepoResponse| match resp {
                MononokeRepoResponse::GetRawFile { content } => Ok(content.to_vec()),
                _ => Err(ErrorKind::InternalError(err_msg(
                    "Actor returned wrong response type to query".to_string(),
                ))),
            })
            .map_err(move |e| GetRawExn::e(e.into()))
            .timed({
                move |stats, resp| {
                    log_time(
                        &mut scuba,
                        &stats,
                        resp,
                        resp.map(|vec| vec.len()).unwrap_or(0),
                    );

                    Ok(())
                }
            })
    }

    fn get_changeset(
        &self,
        params: MononokeGetChangesetParams,
    ) -> BoxFuture<MononokeChangeset, GetChangesetExn> {
        let ctx = self.create_ctx();

        let mut scuba = self.create_scuba_logger(
            "get_changeset",
            &params,
            None,
            Some(params.revision.clone()),
        );

        params
            .try_into()
            .into_future()
            .from_err()
            .and_then({
                cloned!(self.addr);
                move |param| addr.send_query(ctx, param)
            })
            .and_then(|resp: MononokeRepoResponse| match resp {
                MononokeRepoResponse::GetChangeset { changeset } => {
                    Ok(MononokeChangeset::from(changeset))
                }
                _ => Err(ErrorKind::InternalError(err_msg(
                    "Actor returned wrong response type to query".to_string(),
                ))),
            })
            .map_err(move |e| GetChangesetExn::e(e.into()))
            .timed({
                move |stats, resp| {
                    log_time(
                        &mut scuba,
                        &stats,
                        resp,
                        resp.map(|resp| {
                            resp.commit_hash.as_bytes().len()
                                + resp.message.len()
                                + resp.author.as_bytes().len()
                                + 8 // 8 bytes for the date as i64
                        })
                        .unwrap_or(0),
                    );

                    Ok(())
                }
            })
    }

    fn get_branches(
        &self,
        params: MononokeGetBranchesParams,
    ) -> BoxFuture<MononokeBranches, GetBranchesExn> {
        let ctx = self.create_ctx();

        let mut scuba = self.create_scuba_logger("get_branches", &params, None, None);

        params
            .try_into()
            .into_future()
            .from_err()
            .and_then({
                cloned!(self.addr);
                move |param| addr.send_query(ctx, param)
            })
            .and_then(|resp: MononokeRepoResponse| match resp {
                MononokeRepoResponse::GetBranches { branches } => Ok(MononokeBranches { branches }),
                _ => Err(ErrorKind::InternalError(err_msg(
                    "Actor returned wrong response type to query".to_string(),
                ))),
            })
            .map_err(move |e| GetBranchesExn::e(e.into()))
            .timed({
                move |stats, resp| {
                    log_time(
                        &mut scuba,
                        &stats,
                        resp,
                        resp.map(|resp| {
                            resp.branches
                                .iter()
                                .map(|(bookmark, hash)| bookmark.len() + hash.len())
                                .sum()
                        })
                        .unwrap_or(0),
                    );

                    Ok(())
                }
            })
    }

    fn list_directory(
        &self,
        params: MononokeListDirectoryParams,
    ) -> BoxFuture<MononokeDirectory, ListDirectoryExn> {
        let ctx = self.create_ctx();

        let mut scuba = self.create_scuba_logger(
            "list_directory",
            &params,
            Some(params.path.clone()),
            Some(params.revision.clone()),
        );

        params
            .try_into()
            .into_future()
            .from_err()
            .and_then({
                cloned!(self.addr);
                move |param| addr.send_query(ctx, param)
            })
            .and_then(|resp: MononokeRepoResponse| match resp {
                MononokeRepoResponse::ListDirectory { files } => Ok(MononokeDirectory {
                    files: files.into_iter().map(|f| f.into()).collect(),
                }),
                _ => Err(ErrorKind::InternalError(err_msg(
                    "Actor returned wrong response type to query".to_string(),
                ))),
            })
            .map_err(move |e| ListDirectoryExn::e(e.into()))
            .timed({
                move |stats, resp| {
                    log_time(
                        &mut scuba,
                        &stats,
                        resp,
                        resp.map(|resp| {
                            resp.files
                                .iter()
                                .map(
                                    |file| file.name.len() + 1, // 1 byte for the filetype
                                )
                                .sum()
                        })
                        .unwrap_or(0),
                    );

                    Ok(())
                }
            })
    }

    fn is_ancestor(&self, params: MononokeIsAncestorParams) -> BoxFuture<bool, IsAncestorExn> {
        let ctx = self.create_ctx();

        let mut scuba = self.create_scuba_logger(
            "is_ancestor",
            &params,
            None,
            Some(params.descendant.clone()),
        );
        let ancestor = match params.ancestor.clone() {
            MononokeRevision::commit_hash(hash) => hash,
            MononokeRevision::bookmark(bookmark) => bookmark,
            UnknownField(_) => "Not a valid MononokeRevision".to_string(),
        };

        scuba.add("ancestor", ancestor);

        params
            .try_into()
            .into_future()
            .from_err()
            .and_then({
                cloned!(self.addr, ctx);
                move |param| addr.send_query(ctx, param)
            })
            .and_then(|resp: MononokeRepoResponse| match resp {
                MononokeRepoResponse::IsAncestor { answer } => Ok(answer),
                _ => Err(ErrorKind::InternalError(err_msg(
                    "Actor returned wrong response type to query".to_string(),
                ))),
            })
            .map_err(move |e| IsAncestorExn::e(e.into()))
            .timed({
                move |stats, resp| {
                    if let Ok(counters) = serde_json::to_string(&ctx.perf_counters()) {
                        scuba.add("extra_context", counters);
                    };
                    log_time(&mut scuba, &stats, resp, resp.map(|_| 0).unwrap_or(0));

                    Ok(())
                }
            })
    }

    fn get_blob(&self, params: MononokeGetBlobParams) -> BoxFuture<MononokeBlob, GetBlobExn> {
        let ctx = self.create_ctx();

        let mut scuba = self.create_scuba_logger("get_blob", &params, None, None);

        params
            .try_into()
            .into_future()
            .from_err()
            .and_then({
                cloned!(self.addr);
                move |param| addr.send_query(ctx, param)
            })
            .and_then(|resp: MononokeRepoResponse| match resp {
                MononokeRepoResponse::GetBlobContent { content } => Ok(MononokeBlob {
                    content: content.to_vec(),
                }),
                _ => Err(ErrorKind::InternalError(err_msg(
                    "Actor returned wrong response type to query".to_string(),
                ))),
            })
            .map_err(move |e| GetBlobExn::e(e.into()))
            .timed({
                move |stats, resp| {
                    log_time(
                        &mut scuba,
                        &stats,
                        resp,
                        resp.map(|resp| resp.content.len()).unwrap_or(0),
                    );

                    Ok(())
                }
            })
    }

    fn get_tree(&self, params: MononokeGetTreeParams) -> BoxFuture<MononokeDirectory, GetTreeExn> {
        let ctx = self.create_ctx();

        let mut scuba = self.create_scuba_logger("get_tree", &params, None, None);

        params
            .try_into()
            .into_future()
            .from_err()
            .and_then({
                cloned!(self.addr);
                move |param| addr.send_query(ctx, param)
            })
            .and_then(|resp: MononokeRepoResponse| match resp {
                MononokeRepoResponse::GetTree { files } => Ok(MononokeDirectory {
                    files: files.into_iter().map(|f| f.into()).collect(),
                }),
                _ => Err(ErrorKind::InternalError(err_msg(
                    "Actor returned wrong response type to query".to_string(),
                ))),
            })
            .map_err(move |e| GetTreeExn::e(e.into()))
            .timed({
                move |stats, resp| {
                    log_time(
                        &mut scuba,
                        &stats,
                        resp,
                        resp.map(|resp| {
                            resp.files
                                .iter()
                                .map(|file| {
                                    file.name.len()
                                        + 1   // FileType
                                        + file.hash.hash.len()
                                        + file.size.as_ref().map(|_| size_of::<usize>()).unwrap_or(0)
                                        + file.content_sha1.as_ref().map(|sha1| sha1.len()).unwrap_or(0)
                                })
                                .sum()
                        })
                        .unwrap_or(0),
                    );

                    Ok(())
                }
            })
    }
}

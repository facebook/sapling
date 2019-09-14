// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{convert::TryFrom, convert::TryInto, mem::size_of, sync::Arc};

use crate::errors::ErrorKind;
use apiserver_thrift::server::MononokeApiservice;
use apiserver_thrift::services::mononoke_apiservice::{
    GetBlobExn, GetBranchesExn, GetChangesetExn, GetFileHistoryExn, GetLastCommitOnPathExn,
    GetRawExn, GetTreeExn, IsAncestorExn, ListDirectoryExn, ListDirectoryUnodesExn,
};
use apiserver_thrift::types::{
    MononokeAPIException, MononokeBlob, MononokeBranches, MononokeChangeset, MononokeDirectory,
    MononokeDirectoryUnodes, MononokeFileHistory, MononokeGetBlobParams, MononokeGetBranchesParams,
    MononokeGetChangesetParams, MononokeGetFileHistoryParams, MononokeGetLastCommitOnPathParams,
    MononokeGetRawParams, MononokeGetTreeParams, MononokeIsAncestorParams,
    MononokeListDirectoryParams, MononokeListDirectoryUnodesParams, MononokeRevision,
};
use apiserver_thrift::MononokeRevision::UnknownField;
use async_trait::async_trait;
use cloned::cloned;
use context::CoreContext;
use failure::{err_msg, Error};
use fbinit::FacebookInit;
use futures::{Future, IntoFuture, Stream};
use futures_ext::FutureExt;
use futures_preview::compat::Future01CompatExt;
use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use sshrelay::SshEnvVars;
use tracing::TraceContext;
use uuid::Uuid;

use super::super::actor::{Mononoke, MononokeQuery, MononokeRepoResponse};

#[derive(Clone)]
pub struct MononokeAPIServiceImpl {
    fb: FacebookInit,
    addr: Arc<Mononoke>,
    logger: Logger,
    scuba_builder: ScubaSampleBuilder,
}

impl MononokeAPIServiceImpl {
    pub fn new(
        fb: FacebookInit,
        addr: Arc<Mononoke>,
        logger: Logger,
        scuba_builder: ScubaSampleBuilder,
    ) -> Self {
        Self {
            fb,
            addr,
            logger,
            scuba_builder,
        }
    }

    fn create_scuba_logger(
        &self,
        path: Option<Vec<u8>>,
        revision: Option<MononokeRevision>,
        reponame: String,
    ) -> ScubaSampleBuilder {
        let mut scuba = self.scuba_builder.clone();
        scuba
            .add_common_server_data()
            .add("type", "thrift")
            .add("reponame", reponame);

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

    async fn convert_and_call<Params, Mapper, Mapped, Ret, Err>(
        &self,
        ctx: CoreContext,
        params: Params,
        mapper: Mapper,
    ) -> Result<Ret, Err>
    where
        Mapper: FnMut(MononokeRepoResponse) -> Mapped,
        MononokeQuery: TryFrom<Params, Error = Error>,
        Err: From<MononokeAPIException>,
        Mapped: IntoFuture<Item = Ret, Error = ErrorKind>,
    {
        params
            .try_into()
            .into_future()
            .from_err()
            .and_then({
                cloned!(self.addr, ctx);
                move |param| addr.send_query(ctx, param)
            })
            .and_then(mapper)
            .map_err(MononokeAPIException::from)
            .map_err(Err::from)
            .compat()
            .await
    }

    fn create_ctx(&self, mut scuba: ScubaSampleBuilder) -> CoreContext {
        let uuid = Uuid::new_v4();
        scuba.add("session_uuid", uuid.to_string());

        CoreContext::new(
            self.fb,
            uuid,
            self.logger.clone(),
            scuba,
            None,
            TraceContext::default(),
            None,
            SshEnvVars::default(),
            None,
        )
    }
}

fn log_response_size(mut scuba: ScubaSampleBuilder, size: usize) {
    scuba.add("response_size", size);
    scuba.add("log_tag", "Thrift request finished");
    scuba.log()
}

#[async_trait]
impl MononokeApiservice for MononokeAPIServiceImpl {
    async fn get_raw(&self, params: MononokeGetRawParams) -> Result<Vec<u8>, GetRawExn> {
        let scuba = self.create_scuba_logger(
            Some(params.path.clone()),
            Some(params.revision.clone()),
            params.repo.clone(),
        );
        let ctx = self.create_ctx(scuba);

        let resp = self
            .convert_and_call(
                ctx.clone(),
                params,
                move |resp: MononokeRepoResponse| match resp {
                    MononokeRepoResponse::GetRawFile(stream) => stream
                        .into_bytes_stream()
                        .from_err()
                        .concat2()
                        .map(|bytes| bytes.to_vec())
                        .left_future(),
                    _ => Err(ErrorKind::InternalError(err_msg(
                        "Actor returned wrong response type to query".to_string(),
                    )))
                    .into_future()
                    .right_future(),
                },
            )
            .await;

        log_response_size(
            ctx.scuba().clone(),
            resp.as_ref().map(Vec::len).unwrap_or(0),
        );
        resp
    }

    async fn get_changeset(
        &self,
        params: MononokeGetChangesetParams,
    ) -> Result<MononokeChangeset, GetChangesetExn> {
        let scuba =
            self.create_scuba_logger(None, Some(params.revision.clone()), params.repo.clone());
        let ctx = self.create_ctx(scuba);

        let resp = self
            .convert_and_call(
                ctx.clone(),
                params,
                |resp: MononokeRepoResponse| match resp {
                    MononokeRepoResponse::GetChangeset { changeset } => {
                        Ok(MononokeChangeset::from(changeset))
                    }
                    _ => Err(ErrorKind::InternalError(err_msg(
                        "Actor returned wrong response type to query".to_string(),
                    ))),
                },
            )
            .await;

        log_response_size(
            ctx.scuba().clone(),
            resp.as_ref()
                .map(|resp| {
                    resp.commit_hash.as_bytes().len()
                        + resp.message.len()
                        + resp.author.as_bytes().len()
                        + size_of::<i64>()
                })
                .unwrap_or(0),
        );
        resp
    }

    async fn get_branches(
        &self,
        params: MononokeGetBranchesParams,
    ) -> Result<MononokeBranches, GetBranchesExn> {
        let scuba = self.create_scuba_logger(None, None, params.repo.clone());
        let ctx = self.create_ctx(scuba);

        let resp = self
            .convert_and_call(
                ctx.clone(),
                params,
                |resp: MononokeRepoResponse| match resp {
                    MononokeRepoResponse::GetBranches { branches } => {
                        Ok(MononokeBranches { branches })
                    }
                    _ => Err(ErrorKind::InternalError(err_msg(
                        "Actor returned wrong response type to query".to_string(),
                    ))),
                },
            )
            .await;

        log_response_size(
            ctx.scuba().clone(),
            resp.as_ref()
                .map(|resp| {
                    resp.branches
                        .iter()
                        .map(|(bookmark, hash)| bookmark.len() + hash.len())
                        .sum()
                })
                .unwrap_or(0),
        );
        resp
    }

    async fn get_file_history(
        &self,
        params: MononokeGetFileHistoryParams,
    ) -> Result<MononokeFileHistory, GetFileHistoryExn> {
        let scuba = self.create_scuba_logger(
            Some(params.path.clone()),
            Some(params.revision.clone()),
            params.repo.clone(),
        );
        let ctx = self.create_ctx(scuba);

        let resp = self
            .convert_and_call(
                ctx.clone(),
                params,
                |resp: MononokeRepoResponse| match resp {
                    MononokeRepoResponse::GetFileHistory { history } => Ok(MononokeFileHistory {
                        history: history
                            .into_iter()
                            .map(|commit| MononokeChangeset::from(commit))
                            .collect(),
                    }),
                    _ => Err(ErrorKind::InternalError(err_msg(
                        "Actor returned wrong response type to query".to_string(),
                    ))),
                },
            )
            .await;

        log_response_size(
            ctx.scuba().clone(),
            resp.as_ref()
                .map(|resp| {
                    resp.history
                        .iter()
                        .map(|commit| {
                            commit.commit_hash.as_bytes().len()
                                + commit.message.len()
                                + commit.author.as_bytes().len()
                                + size_of::<i64>()
                        })
                        .sum()
                })
                .unwrap_or(0),
        );
        resp
    }

    async fn get_last_commit_on_path(
        &self,
        params: MononokeGetLastCommitOnPathParams,
    ) -> Result<MononokeChangeset, GetLastCommitOnPathExn> {
        let scuba = self.create_scuba_logger(
            Some(params.path.clone()),
            Some(params.revision.clone()),
            params.repo.clone(),
        );
        let ctx = self.create_ctx(scuba);

        let resp = self
            .convert_and_call(
                ctx.clone(),
                params,
                |resp: MononokeRepoResponse| match resp {
                    MononokeRepoResponse::GetLastCommitOnPath { commit } => {
                        Ok(MononokeChangeset::from(commit))
                    }
                    _ => Err(ErrorKind::InternalError(err_msg(
                        "Actor returned wrong response type to query".to_string(),
                    ))),
                },
            )
            .await;

        log_response_size(
            ctx.scuba().clone(),
            resp.as_ref()
                .map(|resp| {
                    resp.commit_hash.as_bytes().len()
                        + resp.message.len()
                        + resp.author.as_bytes().len()
                        + size_of::<i64>()
                })
                .unwrap_or(0),
        );
        resp
    }

    async fn list_directory(
        &self,
        params: MononokeListDirectoryParams,
    ) -> Result<MononokeDirectory, ListDirectoryExn> {
        let scuba = self.create_scuba_logger(
            Some(params.path.clone()),
            Some(params.revision.clone()),
            params.repo.clone(),
        );
        let ctx = self.create_ctx(scuba);

        let resp = self
            .convert_and_call(
                ctx.clone(),
                params,
                |resp: MononokeRepoResponse| match resp {
                    MononokeRepoResponse::ListDirectory { files } => Ok(MononokeDirectory {
                        files: files.into_iter().map(|f| f.into()).collect(),
                    }),
                    _ => Err(ErrorKind::InternalError(err_msg(
                        "Actor returned wrong response type to query".to_string(),
                    ))),
                },
            )
            .await;

        log_response_size(
            ctx.scuba().clone(),
            resp.as_ref()
                .map(|resp| {
                    resp.files
                        .iter()
                        .map(
                            |file| file.name.len() + 1, // 1 byte for the filetype
                        )
                        .sum()
                })
                .unwrap_or(0),
        );
        resp
    }

    async fn list_directory_unodes(
        &self,
        params: MononokeListDirectoryUnodesParams,
    ) -> Result<MononokeDirectoryUnodes, ListDirectoryUnodesExn> {
        let scuba = self.create_scuba_logger(
            Some(params.path.clone()),
            Some(params.revision.clone()),
            params.repo.clone(),
        );
        let ctx = self.create_ctx(scuba);

        let resp = self
            .convert_and_call(
                ctx.clone(),
                params,
                |resp: MononokeRepoResponse| match resp {
                    MononokeRepoResponse::ListDirectoryUnodes { files } => {
                        Ok(MononokeDirectoryUnodes {
                            entries: files.into_iter().map(|f| f.into()).collect(),
                        })
                    }
                    _ => Err(ErrorKind::InternalError(err_msg(
                        "Actor returned wrong response type to query".to_string(),
                    ))),
                },
            )
            .await;

        log_response_size(
            ctx.scuba().clone(),
            resp.as_ref()
                .map(|resp| resp.entries.iter().map(|file| file.name.len()).sum())
                .unwrap_or(0),
        );
        resp
    }

    async fn is_ancestor(&self, params: MononokeIsAncestorParams) -> Result<bool, IsAncestorExn> {
        let mut scuba =
            self.create_scuba_logger(None, Some(params.descendant.clone()), params.repo.clone());

        let ancestor = match params.ancestor.clone() {
            MononokeRevision::commit_hash(hash) => hash,
            MononokeRevision::bookmark(bookmark) => bookmark,
            UnknownField(_) => "Not a valid MononokeRevision".to_string(),
        };

        scuba.add("ancestor", ancestor);
        let ctx = self.create_ctx(scuba);

        let resp = self
            .convert_and_call(
                ctx.clone(),
                params,
                |resp: MononokeRepoResponse| match resp {
                    MononokeRepoResponse::IsAncestor { answer } => Ok(answer),
                    _ => Err(ErrorKind::InternalError(err_msg(
                        "Actor returned wrong response type to query".to_string(),
                    ))),
                },
            )
            .await;

        log_response_size(ctx.scuba().clone(), 0);
        resp
    }

    async fn get_blob(&self, params: MononokeGetBlobParams) -> Result<MononokeBlob, GetBlobExn> {
        let scuba = self.create_scuba_logger(None, None, params.repo.clone());
        let ctx = self.create_ctx(scuba);

        let resp = self
            .convert_and_call(
                ctx.clone(),
                params,
                |resp: MononokeRepoResponse| match resp {
                    MononokeRepoResponse::GetBlobContent(stream) => stream
                        .into_bytes_stream()
                        .from_err()
                        .concat2()
                        .map(|bytes| bytes.to_vec())
                        .map(|content| MononokeBlob { content })
                        .left_future(),
                    _ => Err(ErrorKind::InternalError(err_msg(
                        "Actor returned wrong response type to query".to_string(),
                    )))
                    .into_future()
                    .right_future(),
                },
            )
            .await;

        log_response_size(
            ctx.scuba().clone(),
            resp.as_ref().map(|resp| resp.content.len()).unwrap_or(0),
        );
        resp
    }

    async fn get_tree(
        &self,
        params: MononokeGetTreeParams,
    ) -> Result<MononokeDirectory, GetTreeExn> {
        let scuba = self.create_scuba_logger(None, None, params.repo.clone());
        let ctx = self.create_ctx(scuba);

        let resp = self
            .convert_and_call(
                ctx.clone(),
                params,
                |resp: MononokeRepoResponse| match resp {
                    MononokeRepoResponse::GetTree { files } => Ok(MononokeDirectory {
                        files: files.into_iter().map(|f| f.into()).collect(),
                    }),
                    _ => Err(ErrorKind::InternalError(err_msg(
                        "Actor returned wrong response type to query".to_string(),
                    ))),
                },
            )
            .await;

        log_response_size(
            ctx.scuba().clone(),
            resp.as_ref()
                .map(|resp| {
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
        resp
    }
}

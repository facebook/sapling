// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::convert::TryInto;
use std::sync::Arc;

use apiserver_thrift::server::MononokeApiservice;
use apiserver_thrift::services::mononoke_apiservice::{GetBranchesExn, GetChangesetExn, GetRawExn};
use apiserver_thrift::types::{
    MononokeBranches, MononokeChangeset, MononokeGetBranchesParams, MononokeGetChangesetParams,
    MononokeGetRawParams,
};
use errors::ErrorKind;
use failure::err_msg;
use futures::{Future, IntoFuture};
use futures_ext::BoxFuture;
use futures_stats::Timed;
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
    pub fn new(addr: Arc<Mononoke>, logger: Logger, scuba_table_name: Option<String>) -> Self {
        let mut scuba_builder = if let Some(table_name) = scuba_table_name {
            ScubaSampleBuilder::new(table_name)
        } else {
            ScubaSampleBuilder::with_discard()
        };

        scuba_builder.add_common_server_data();

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
        path: Vec<u8>,
        revision: String,
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
            )
            .add(
                "path",
                String::from_utf8(path).unwrap_or("Invalid UTF-8 in path".to_string()),
            )
            .add("revision", revision);
        scuba
    }
}

impl MononokeApiservice for MononokeAPIServiceImpl {
    fn get_raw(&self, params: MononokeGetRawParams) -> BoxFuture<Vec<u8>, GetRawExn> {
        let mut scuba = self.create_scuba_logger(
            "get_raw",
            &params,
            params.path.clone(),
            params.changeset.clone(),
        );

        params
            .try_into()
            .into_future()
            .from_err()
            .and_then({
                cloned!(self.addr);
                move |param| addr.send_query(param)
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
                    scuba
                        .add_future_stats(&stats)
                        .add("response_time", stats.completion_time.as_micros_unchecked())
                        .add("response_size", resp.map(|vec| vec.len()).unwrap_or(0));

                    scuba.log();

                    Ok(())
                }
            })
    }

    fn get_changeset(
        &self,
        params: MononokeGetChangesetParams,
    ) -> BoxFuture<MononokeChangeset, GetChangesetExn> {
        let mut scuba = self.create_scuba_logger(
            "get_changeset",
            &params,
            Vec::new(),
            params.revision.clone(),
        );

        params
            .try_into()
            .into_future()
            .from_err()
            .and_then({
                cloned!(self.addr);
                move |param| addr.send_query(param)
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
                    scuba
                        .add(
                            "response_size",
                            resp.map(|resp| {
                                resp.commit_hash.as_bytes().len()
                                    + resp.message.len()
                                    + resp.author.as_bytes().len()
                                    + 8 // 8 bytes for the date as i64
                            })
                            .unwrap_or(0),
                        )
                        .add_future_stats(&stats)
                        .add("response_time", stats.completion_time.as_micros_unchecked());

                    scuba.log();

                    Ok(())
                }
            })
    }

    fn get_branches(
        &self,
        params: MononokeGetBranchesParams,
    ) -> BoxFuture<MononokeBranches, GetBranchesExn> {
        let mut scuba =
            self.create_scuba_logger("get_branches", &params, Vec::new(), String::new());

        params
            .try_into()
            .into_future()
            .from_err()
            .and_then({
                cloned!(self.addr);
                move |param| addr.send_query(param)
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
                    scuba
                        .add(
                            "response_size",
                            resp.map(|resp| {
                                resp.branches
                                    .iter()
                                    .map(|(bookmark, hash)| bookmark.len() + hash.len())
                                    .sum()
                            })
                            .unwrap_or(0),
                        )
                        .add_future_stats(&stats)
                        .add("response_time", stats.completion_time.as_micros_unchecked());

                    scuba.log();

                    Ok(())
                }
            })
    }
}

// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::convert::TryInto;

use actix::Addr;
use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use slog::Logger;

use apiserver_thrift::server::MononokeApiservice;
use apiserver_thrift::services::mononoke_apiservice::GetRawExn;
use apiserver_thrift::types::MononokeGetRawParams;

use super::super::actor::{unwrap_request, MononokeActor, MononokeRepoResponse};

#[derive(Clone)]
pub struct MononokeAPIServiceImpl {
    addr: Addr<MononokeActor>,
    logger: Logger,
}

impl MononokeAPIServiceImpl {
    pub fn new(addr: Addr<MononokeActor>, logger: Logger) -> Self {
        Self { addr, logger }
    }
}

impl MononokeApiservice for MononokeAPIServiceImpl {
    fn get_raw(&self, params: MononokeGetRawParams) -> BoxFuture<Vec<u8>, GetRawExn> {
        params
            .try_into()
            .into_future()
            .from_err()
            .and_then({
                cloned!(self.addr);
                move |param| unwrap_request(addr.send(param))
            })
            .and_then(|resp: MononokeRepoResponse| match resp {
                MononokeRepoResponse::GetRawFile { content } => Ok(content.to_vec()),
                _ => unreachable!(),
            })
            .map_err(move |e| GetRawExn::e(e.into()))
            .boxify()
    }
}

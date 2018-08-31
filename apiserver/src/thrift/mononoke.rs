// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::convert::TryInto;
use std::sync::Arc;

use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use slog::Logger;

use apiserver_thrift::server::MononokeApiservice;
use apiserver_thrift::services::mononoke_apiservice::GetRawExn;
use apiserver_thrift::types::MononokeGetRawParams;

use super::super::actor::{Mononoke, MononokeRepoResponse};

#[derive(Clone)]
pub struct MononokeAPIServiceImpl {
    addr: Arc<Mononoke>,
    logger: Logger,
}

impl MononokeAPIServiceImpl {
    pub fn new(addr: Arc<Mononoke>, logger: Logger) -> Self {
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
                move |param| addr.send_query(param)
            })
            .and_then(|resp: MononokeRepoResponse| match resp {
                MononokeRepoResponse::GetRawFile { content } => Ok(content.to_vec()),
                _ => unreachable!(),
            })
            .map_err(move |e| GetRawExn::e(e.into()))
            .boxify()
    }
}

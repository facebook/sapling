/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::{Deserialize, Serialize};

use gotham_ext::{error::HttpError, response::BytesBody};

use crate::context::ServerContext;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct ReposParams {
    pretty: Option<bool>,
}

#[derive(Clone, Serialize, Debug)]
struct ReposResponse<'a> {
    repos: Vec<&'a str>,
}

pub fn repos(state: &mut State) -> Result<BytesBody<Bytes>, HttpError> {
    let params = ReposParams::borrow_from(state);
    let sctx = ServerContext::borrow_from(state);
    let mononoke = sctx.mononoke_api();

    let repos = mononoke.repo_names().collect::<Vec<_>>();
    let response = ReposResponse { repos };

    let serialize = match params.pretty {
        Some(true) => serde_json::to_vec_pretty,
        _ => serde_json::to_vec,
    };

    let bytes: Bytes = serialize(&response).map_err(HttpError::e500)?.into();
    Ok(BytesBody::new(bytes, mime::APPLICATION_JSON))
}

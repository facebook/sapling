/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use bytes::Bytes;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::error::HttpError;
use gotham_ext::response::BytesBody;
use serde::Serialize;

use crate::context::ServerContext;
use crate::errors::ErrorKind;

#[derive(Clone, Serialize, Debug)]
struct ReposResponse {
    repos: Vec<String>,
}

pub async fn repos(state: &mut State) -> Result<BytesBody<Bytes>, HttpError> {
    let sctx = ServerContext::borrow_from(state);
    let mononoke = sctx.mononoke_api();

    let repos = mononoke.repo_names().collect::<Vec<_>>();
    let response = ReposResponse { repos };
    let bytes: Bytes = serde_json::to_vec(&response)
        .context(ErrorKind::SerializationFailed)
        .map_err(HttpError::e500)?
        .into();

    Ok(BytesBody::new(bytes, mime::APPLICATION_JSON))
}

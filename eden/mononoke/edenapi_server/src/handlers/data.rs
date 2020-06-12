/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use futures::{stream::FuturesUnordered, TryStreamExt};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;

use edenapi_types::{DataEntry, DataRequest, DataResponse};
use gotham_ext::{error::HttpError, response::TryIntoResponse};
use mercurial_types::HgNodeHash;
use mononoke_api::hg::{HgDataContext, HgDataId, HgRepoContext};
use types::Key;

use crate::context::ServerContext;
use crate::errors::{ErrorKind, MononokeErrorExt};
use crate::middleware::RequestContext;
use crate::utils::{cbor_response, get_repo, parse_cbor_request};

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct DataParams {
    repo: String,
}

pub async fn data<ID: HgDataId>(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let rctx = RequestContext::borrow_from(state);
    let sctx = ServerContext::borrow_from(state);
    let params = DataParams::borrow_from(state);

    let repo = get_repo(&sctx, &rctx, &params.repo).await?;
    let request = parse_cbor_request(state).await?;
    let response = get_all_entries::<ID>(&repo, request).await?;

    cbor_response(response)
}

/// Fetch data for all of the requested keys concurrently.
async fn get_all_entries<ID: HgDataId>(
    repo: &HgRepoContext,
    request: DataRequest,
) -> Result<DataResponse, HttpError> {
    let fetches = FuturesUnordered::new();
    for key in request.keys {
        fetches.push(get_data_entry::<ID>(repo.clone(), key));
    }
    let entries = fetches.try_collect::<Vec<_>>().await?;

    Ok(DataResponse::new(entries))
}

/// Fetch requested data for a single key.
/// Note that this function consumes the repo context in order
/// to construct a file/tree context for the requested blob.
async fn get_data_entry<ID: HgDataId>(
    repo: HgRepoContext,
    key: Key,
) -> Result<DataEntry, HttpError> {
    let id = ID::from_node_hash(HgNodeHash::from(key.hgid));

    let ctx = id
        .context(repo)
        .await
        .map_err(|e| e.into_http_error(ErrorKind::DataFetchFailed(key.clone())))?
        .with_context(|| ErrorKind::KeyDoesNotExist(key.clone()))
        .map_err(HttpError::e404)?;

    let data = ctx.content().await.map_err(HttpError::e500)?;
    let parents = ctx.hg_parents().into();

    Ok(DataEntry::new(key, data, parents))
}

/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use bytes::Bytes;
use futures::{stream::FuturesUnordered, TryStreamExt};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;

use gotham_ext::{error::HttpError, response::BytesBody};
use mercurial_types::HgNodeHash;
use mononoke_api::hg::{HgDataContext, HgDataId, HgRepoContext};
use types::{
    api::{DataRequest, DataResponse},
    DataEntry, Key,
};

use crate::context::ServerContext;
use crate::middleware::RequestContext;

use super::util::{cbor_mime, get_repo, get_request_body};

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct DataParams {
    repo: String,
}

pub async fn data<ID: HgDataId>(state: &mut State) -> Result<BytesBody<Bytes>, HttpError> {
    let rctx = RequestContext::borrow_from(state);
    let sctx = ServerContext::borrow_from(state);
    let params = DataParams::borrow_from(state);

    let repo = get_repo(&sctx, &rctx, &params.repo).await?;
    let body = get_request_body(state).await?;

    let request = serde_cbor::from_slice(&body).map_err(HttpError::e400)?;
    let response = get_all_entries::<ID>(&repo, request).await?;
    let bytes: Bytes = serde_cbor::to_vec(&response)
        .map_err(HttpError::e500)?
        .into();

    Ok(BytesBody::new(bytes, cbor_mime()))
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
        .map_err(HttpError::e500)?
        .ok_or_else(|| HttpError::e404(anyhow!("key not found: {:?}", &key)))?;

    let data = ctx.content().await.map_err(HttpError::e500)?;
    let parents = ctx.hg_parents().into();

    Ok(DataEntry::new(key, data, parents))
}

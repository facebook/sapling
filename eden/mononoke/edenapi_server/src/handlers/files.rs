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
use http::HeaderMap;
use hyper::Body;

use serde::Deserialize;

use gotham_ext::{body_ext::BodyExt, error::HttpError, response::BytesBody};
use mercurial_types::{HgFileNodeId, HgNodeHash};
use mononoke_api::hg::{HgDataContext, HgRepoContext};
use types::{
    api::{DataRequest, DataResponse},
    DataEntry, Key,
};

use crate::context::ServerContext;
use crate::middleware::RequestContext;

use super::util::cbor_mime;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct FilesParams {
    repo: String,
}

pub async fn files(state: &mut State) -> Result<BytesBody<Bytes>, HttpError> {
    let rctx = RequestContext::borrow_from(state);
    let sctx = ServerContext::borrow_from(state);
    let params = FilesParams::borrow_from(state);

    let repo = sctx
        .mononoke_api()
        .repo(rctx.core_context().clone(), &params.repo)
        .await
        .map_err(HttpError::e403)?
        .ok_or_else(|| HttpError::e404(anyhow!("repo does not exist: {:?}", &params.repo)))?;

    let hg_repo = repo.hg();

    let body = Body::take_from(state);
    let headers = HeaderMap::try_borrow_from(state);
    let payload: Bytes = body
        .try_concat_body_opt(headers)
        .map_err(HttpError::e400)?
        .await
        .map_err(HttpError::e400)?;

    let request = serde_cbor::from_slice(&payload).map_err(HttpError::e400)?;
    let response = get_all_files(&hg_repo, request).await?;
    let bytes: Bytes = serde_cbor::to_vec(&response)
        .map_err(HttpError::e500)?
        .into();

    Ok(BytesBody::new(bytes, cbor_mime()))
}

/// Fetch data for all of the requested files concurrently.
async fn get_all_files(
    hg_repo: &HgRepoContext,
    request: DataRequest,
) -> Result<DataResponse, HttpError> {
    let fetches = FuturesUnordered::new();
    for key in request.keys {
        fetches.push(get_file(hg_repo, key));
    }
    let entries = fetches.try_collect::<Vec<_>>().await?;

    Ok(DataResponse::new(entries))
}

/// Fetch requested data for a single file.
async fn get_file(hg_repo: &HgRepoContext, key: Key) -> Result<DataEntry, HttpError> {
    let filenode_id = HgFileNodeId::new(HgNodeHash::from(key.hgid));
    let file = hg_repo
        .file(filenode_id)
        .await
        .map_err(HttpError::e500)?
        .ok_or_else(|| HttpError::e404(anyhow!("file does not exist: {:?}", &key)))?;

    let data = file.content().await.map_err(HttpError::e500)?;
    let parents = file.hg_parents().into();

    Ok(DataEntry::new(key, data, parents))
}

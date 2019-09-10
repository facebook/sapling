// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures_preview::compat::Future01CompatExt;
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use hyper::Body;
use serde::Deserialize;

use filestore::{self, FetchKey};
use mononoke_types::ContentId;

use crate::errors::ErrorKind;
use crate::http::HttpError;
use crate::lfs_server_context::RequestContext;

#[derive(Deserialize, StateData, StaticResponseExtender)]
pub struct DownloadParams {
    repository: String,
    content_id: String,
}

pub async fn download(state: &mut State) -> Result<(Body, mime::Mime), HttpError> {
    let DownloadParams {
        repository,
        content_id,
    } = DownloadParams::borrow_from(state);

    let ctx = RequestContext::instantiate(state, repository.clone()).map_err(HttpError::e400)?;

    let content_id = ContentId::from_str(&content_id).map_err(HttpError::e400)?;

    // Query a stream out of the Filestore
    let stream = filestore::fetch(
        &ctx.repo.get_blobstore(),
        ctx.ctx.clone(),
        &FetchKey::Canonical(content_id),
    )
    .compat()
    .await
    .map_err(HttpError::e500)?;

    // Return a 404 if the stream doesn't exist.
    let stream = stream
        .ok_or_else(|| ErrorKind::ObjectDoesNotExist(content_id))
        .map_err(HttpError::e404)?;

    // NOTE: This is a Futures 0.1 stream ... which is what Hyper wants here (for now).
    let body = Body::wrap_stream(stream);

    Ok((body, mime::APPLICATION_OCTET_STREAM))
}

// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures_preview::compat::Future01CompatExt;
use gotham::{
    handler::IntoHandlerError,
    helpers::http::response::create_response,
    state::{FromState, State},
};
use gotham_derive::{StateData, StaticResponseExtender};
use hyper::{Body, StatusCode};
use mime;
use serde::Deserialize;

use filestore::{self, FetchKey};
use mononoke_types::ContentId;

use crate::errors::ErrorKind;
use crate::http::{git_lfs_mime, HandlerResponse};
use crate::lfs_server_context::RequestContext;
use crate::protocol::ResponseError;
use crate::{bail_http_400, bail_http_404, bail_http_500};

#[derive(Deserialize, StateData, StaticResponseExtender)]
pub struct DownloadParams {
    repository: String,
    content_id: String,
}

pub async fn download(state: State) -> HandlerResponse {
    let DownloadParams {
        repository,
        content_id,
    } = DownloadParams::borrow_from(&state);

    let ctx = bail_http_400!(
        state,
        RequestContext::instantiate(&state, repository.clone())
    );
    let content_id = bail_http_400!(state, ContentId::from_str(&content_id));

    // Query a stream out of the Filestore
    let stream = filestore::fetch(
        &ctx.repo.get_blobstore(),
        ctx.ctx.clone(),
        &FetchKey::Canonical(content_id),
    )
    .compat()
    .await;
    let stream = bail_http_500!(state, stream);

    // Return a 404 if the stream doesn't exist.
    let stream = stream.ok_or_else(|| ErrorKind::ObjectDoesNotExist(content_id));
    let stream = bail_http_404!(state, stream);

    // Got a stream, let's return!
    // NOTE: This is a Futures 0.1 stream ... which is what Hyper wants here (for now).
    let body = Body::wrap_stream(stream);
    let res = create_response(&state, StatusCode::OK, mime::APPLICATION_OCTET_STREAM, body);

    Ok((state, res))
}

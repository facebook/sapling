// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure_ext::chain::ChainExt;
use futures_preview::compat::Future01CompatExt;
use gotham::state::State;
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;

use filestore::{self, FetchKey};
use mononoke_types::ContentId;
use stats::{define_stats, Histogram};

use crate::errors::ErrorKind;
use crate::http::{HttpError, StreamBody, TryIntoResponse};
use crate::lfs_server_context::RequestContext;

const METHOD: &str = "download";

define_stats! {
    prefix ="mononoke.lfs.download";
    size_bytes: histogram(1_500_000, 0, 150_000_000, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
}

#[derive(Deserialize, StateData, StaticResponseExtender)]
pub struct DownloadParams {
    repository: String,
    content_id: String,
}

pub async fn download(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let DownloadParams {
        repository,
        content_id,
    } = state.take();

    let ctx =
        RequestContext::instantiate(state, repository.clone(), METHOD).map_err(HttpError::e400)?;

    let content_id = ContentId::from_str(&content_id)
        .chain_err(ErrorKind::InvalidContentId)
        .map_err(HttpError::e400)?;

    // Query a stream out of the Filestore
    let fetch_stream = filestore::fetch_with_size(
        &ctx.repo.get_blobstore(),
        ctx.ctx.clone(),
        &FetchKey::Canonical(content_id),
    )
    .compat()
    .await
    .chain_err(ErrorKind::FilestoreReadFailure)
    .map_err(HttpError::e500)?;

    // Return a 404 if the stream doesn't exist.
    let (stream, size) = fetch_stream
        .ok_or_else(|| ErrorKind::ObjectDoesNotExist(content_id))
        .map_err(HttpError::e404)?;

    STATS::size_bytes.add_value(size as i64);

    Ok(StreamBody::new(
        stream,
        size,
        mime::APPLICATION_OCTET_STREAM,
    ))
}

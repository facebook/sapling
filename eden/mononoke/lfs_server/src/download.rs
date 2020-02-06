/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use mononoke_types::hash::Sha256;

use failure_ext::chain::ChainExt;
use futures::Stream;
use futures_ext::StreamExt;
use futures_preview::compat::Future01CompatExt;
use gotham::state::State;
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;
use std::str::FromStr;

use filestore::{self, Alias, FetchKey};
use mononoke_types::ContentId;
use stats::prelude::*;

use crate::errors::ErrorKind;
use crate::http::{HttpError, StreamBody, TryIntoResponse};
use crate::lfs_server_context::RepositoryRequestContext;
use crate::middleware::LfsMethod;

define_stats! {
    prefix = "mononoke.lfs.download";
    size_bytes_sent: timeseries(
        "size_bytes_sent";
        Sum;
        Duration::from_secs(5), Duration::from_secs(15), Duration::from_secs(60)
    ),
}

#[derive(Deserialize, StateData, StaticResponseExtender)]
pub struct DownloadParamsContentId {
    repository: String,
    content_id: String,
}

#[derive(Deserialize, StateData, StaticResponseExtender)]
pub struct DownloadParamsSha256 {
    repository: String,
    oid: String,
}

async fn fetch_by_key(
    ctx: RepositoryRequestContext,
    key: FetchKey,
) -> Result<impl TryIntoResponse, HttpError> {
    // Query a stream out of the Filestore
    let fetch_stream = filestore::fetch_with_size(ctx.repo.blobstore(), ctx.ctx.clone(), &key)
        .compat()
        .await
        .chain_err(ErrorKind::FilestoreReadFailure)
        .map_err(HttpError::e500)?;

    // Return a 404 if the stream doesn't exist.
    let (stream, size) = fetch_stream
        .ok_or_else(|| ErrorKind::ObjectDoesNotExist(key))
        .map_err(HttpError::e404)?;

    let stream = if ctx.config.track_bytes_sent {
        stream
            .inspect(|bytes| STATS::size_bytes_sent.add_value(bytes.len() as i64))
            .left_stream()
    } else {
        stream.right_stream()
    };

    Ok(StreamBody::new(
        stream,
        size,
        mime::APPLICATION_OCTET_STREAM,
    ))
}

pub async fn download(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let DownloadParamsContentId {
        repository,
        content_id,
    } = state.take();

    let content_id = ContentId::from_str(&content_id)
        .chain_err(ErrorKind::InvalidContentId)
        .map_err(HttpError::e400)?;

    let key = FetchKey::Canonical(content_id);

    let ctx =
        RepositoryRequestContext::instantiate(state, repository.clone(), LfsMethod::Download)?;

    fetch_by_key(ctx, key).await
}

pub async fn download_sha256(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let DownloadParamsSha256 { repository, oid } = state.take();

    let oid = Sha256::from_str(&oid)
        .chain_err(ErrorKind::InvalidOid)
        .map_err(HttpError::e400)?;

    let key = FetchKey::Aliased(Alias::Sha256(oid));

    let ctx = RepositoryRequestContext::instantiate(
        state,
        repository.clone(),
        LfsMethod::DownloadSha256,
    )?;

    fetch_by_key(ctx, key).await
}

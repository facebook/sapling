/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use bytes::Bytes;
use filestore::FilestoreConfigRef;
use filestore::StoreRequest;
use futures::Stream;
use futures::TryStreamExt;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use gotham_derive::StaticResponseExtender;
use gotham_ext::error::HttpError;
use gotham_ext::middleware::HttpScubaKey;
use gotham_ext::middleware::ScubaMiddlewareState;
use gotham_ext::response::EmptyBody;
use gotham_ext::response::TryIntoResponse;
use http::header::CONTENT_LENGTH;
use hyper::Body;
use mononoke_types::hash::GitSha1;
use mononoke_types::hash::RichGitSha1;
use repo_blobstore::RepoBlobstoreRef;
use serde::Deserialize;
use stats::prelude::*;
use std::str::FromStr;

use crate::errors::ErrorKind;
use crate::lfs_server_context::RepositoryRequestContext;
use crate::middleware::LfsMethod;
use crate::util::read_header_value;

define_stats! {
    prefix ="mononoke.lfs.git_upload_blob";
    total_uploads: timeseries(Rate, Sum),
    upload_success: timeseries(Rate, Sum),
    size_bytes: histogram(100_000, 0, 10_000_000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
}

// NOTE: We don't deserialize things beyond a String form, in order to report errors in our
// controller, not in routing.
#[derive(Deserialize, StateData, StaticResponseExtender)]
pub struct GitBlobParams {
    repository: String,
    oid: String,
    size: String,
}

async fn upload_blob<S>(
    ctx: &RepositoryRequestContext,
    oid: RichGitSha1,
    size: u64,
    body: S,
) -> Result<(), Error>
where
    S: Stream<Item = Result<Bytes, ()>> + Unpin + Send + 'static,
{
    STATS::total_uploads.add_value(1);

    filestore::store(
        ctx.repo.repo_blobstore(),
        *ctx.repo.filestore_config(),
        &ctx.ctx,
        &StoreRequest::with_git_sha1(size, oid),
        body.map_err(|()| ErrorKind::ClientCancelled).err_into(),
    )
    .await
    .context(ErrorKind::FilestoreWriteFailure)?;

    STATS::upload_success.add_value(1);

    Ok(())
}

pub async fn git_upload_blob(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let GitBlobParams {
        repository,
        oid,
        size,
    } = state.take();

    let ctx = RepositoryRequestContext::instantiate(state, repository.clone(), LfsMethod::GitBlob)
        .await?;

    let oid = GitSha1::from_str(&oid).map_err(HttpError::e400)?;
    let size = size.parse().map_err(Error::from).map_err(HttpError::e400)?;
    let content_length: Option<u64> = read_header_value(state, CONTENT_LENGTH)
        .transpose()
        .map_err(HttpError::e400)?;

    if let Some(content_length) = content_length {
        ScubaMiddlewareState::try_borrow_add(
            state,
            HttpScubaKey::RequestContentLength,
            content_length,
        );
    }

    STATS::size_bytes.add_value(size as i64);

    if let Some(max_upload_size) = ctx.max_upload_size() {
        if size > max_upload_size {
            return Err(HttpError::e400(ErrorKind::UploadTooLarge(
                size,
                max_upload_size,
            )));
        }
    }

    let oid = RichGitSha1::from_sha1(oid, "blob", size);
    let body = Body::take_from(state).map_err(|_| ());
    upload_blob(&ctx, oid, size, body)
        .await
        .map_err(HttpError::e500)?;

    Ok(EmptyBody::new())
}

/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::str::{self, FromStr};

use anyhow::{Context, Error};
use bytes::Bytes;
use futures::{
    channel::mpsc::{self, channel},
    compat::{Future01CompatExt, Stream01CompatExt},
    future::ready,
    SinkExt, Stream, StreamExt, TryStreamExt,
};
use futures_util::try_join;
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use http::header::{HeaderMap, CONTENT_LENGTH};
use hyper::{Body, Request};
use serde::Deserialize;
use stats::prelude::*;

use filestore::{self, Alias, FetchKey, StoreRequest};
use gotham_ext::{
    error::HttpError,
    response::{EmptyBody, TryIntoResponse},
};
use lfs_protocol::{
    ObjectAction, ObjectStatus, Operation, RequestBatch, RequestObject, ResponseBatch,
    Sha256 as LfsSha256, Transfer,
};
use mononoke_types::hash::Sha256;

use crate::errors::ErrorKind;
use crate::lfs_server_context::RepositoryRequestContext;
use crate::middleware::{LfsMethod, ScubaKey, ScubaMiddlewareState};

define_stats! {
    prefix ="mononoke.lfs.upload";
    upstream_uploads: timeseries(Rate, Sum),
    upstream_success: timeseries(Rate, Sum),
    internal_uploads: timeseries(Rate, Sum),
    internal_success: timeseries(Rate, Sum),
    size_bytes: histogram(1_500_000, 0, 150_000_000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
}

// Small buffers for Filestore & Dewey
const BUFFER_SIZE: usize = 5;

// NOTE: We don't deserialize things beyond a String form, in order to report errors in our
// controller, not in routing.
#[derive(Deserialize, StateData, StaticResponseExtender)]
pub struct UploadParams {
    repository: String,
    oid: String,
    size: String,
}

fn find_actions(
    batch: ResponseBatch,
    object: &RequestObject,
) -> Result<HashMap<Operation, ObjectAction>, Error> {
    let ResponseBatch { transfer, objects } = batch;

    match transfer {
        Transfer::Basic => objects
            .into_iter()
            .find(|o| o.object == *object)
            .ok_or(ErrorKind::UpstreamMissingObject(*object).into())
            .and_then(|o| match o.status {
                ObjectStatus::Ok {
                    authenticated: false,
                    actions,
                } => Ok(actions),
                _ => Err(ErrorKind::UpstreamInvalidObject(o).into()),
            }),
        Transfer::Unknown => Err(ErrorKind::UpstreamInvalidTransfer.into()),
    }
}

// TODO: The reason we have discard_stream instead of just droppping the Stream is because we need
// to make sure we don't drop our receiver and break our sender in the multiplexing that happens
// below. This is OK when the data is coming from a client and is going to be there anyway, but
// it's still a bit crusty. A better way to do this would be to wrap the Sink somehow to ignore
// errors sending.
async fn discard_stream<S>(data: S) -> Result<(), Error>
where
    S: Stream<Item = Result<Bytes, Error>> + Unpin + Send + 'static,
{
    Ok(data.for_each(|_| ready(())).await)
}

async fn internal_upload<S>(
    ctx: &RepositoryRequestContext,
    oid: Sha256,
    size: u64,
    data: S,
) -> Result<(), Error>
where
    S: Stream<Item = Result<Bytes, Error>> + Unpin + Send + 'static,
{
    STATS::internal_uploads.add_value(1);

    let res = filestore::store(
        ctx.repo.get_blobstore(),
        ctx.repo.filestore_config(),
        ctx.ctx.clone(),
        &StoreRequest::with_sha256(size, oid),
        data.compat(),
    )
    .compat()
    .await
    .context(ErrorKind::FilestoreWriteFailure)
    .map_err(Error::from);

    if !res.is_err() {
        STATS::internal_success.add_value(1);
    }

    Ok(())
}

async fn upstream_upload<S>(
    ctx: &RepositoryRequestContext,
    oid: Sha256,
    size: u64,
    data: S,
) -> Result<(), Error>
where
    S: Stream<Item = Result<Bytes, Error>> + Unpin + Send + 'static,
{
    let object = RequestObject {
        oid: LfsSha256(oid.into_inner()),
        size,
    };

    let batch = RequestBatch {
        operation: Operation::Upload,
        r#ref: None,
        transfers: vec![Transfer::Basic],
        objects: vec![object],
    };

    let res = ctx
        .upstream_batch(&batch)
        .await
        .context(ErrorKind::UpstreamBatchError)?;

    let batch = match res {
        Some(res) => res,
        None => {
            // We have no upstream: discard this copy.
            return discard_stream(data).await;
        }
    };

    let action = find_actions(batch, &object)?.remove(&Operation::Upload);

    if let Some(action) = action {
        // TODO: We are discarding expiry and headers here. We probably shouldn't.
        // TODO: We are discarding verify actions.
        STATS::upstream_uploads.add_value(1);
        let ObjectAction { href, .. } = action;

        // TODO: Fix this after updating Hyper: https://github.com/hyperium/hyper/pull/2187
        let (sender, receiver) = mpsc::channel(0);
        tokio::spawn(data.map(Ok).forward(sender));

        let body = Body::wrap_stream(receiver);
        let req = Request::put(href)
            .header("Content-Length", &size.to_string())
            .body(body.into())?;

        // NOTE: We read the response body here, otherwise Hyper will not allow this connection to
        // be reused.
        ctx.dispatch(req)
            .await
            .context(ErrorKind::UpstreamUploadError)?
            .discard()
            .await?;

        STATS::upstream_success.add_value(1);
        return Ok(());
    }

    discard_stream(data).await
}

async fn upload_from_client(
    ctx: &RepositoryRequestContext,
    oid: Sha256,
    size: u64,
    state: &mut State,
) -> Result<(), Error> {
    let (internal_send, internal_recv) = channel::<Result<Bytes, ()>>(BUFFER_SIZE);
    let (upstream_send, upstream_recv) = channel::<Result<Bytes, ()>>(BUFFER_SIZE);

    let mut sink = internal_send.fanout(upstream_send);

    let internal_recv = internal_recv
        .map_err(|()| ErrorKind::ClientCancelled)
        .err_into();

    let upstream_recv = upstream_recv
        .map_err(|()| ErrorKind::ClientCancelled)
        .err_into();

    let internal_upload = internal_upload(&ctx, oid, size, internal_recv);
    let upstream_upload = upstream_upload(&ctx, oid, size, upstream_recv);

    let mut received: usize = 0;

    let mut data = Body::take_from(state)
        .map_ok(|chunk| {
            received += chunk.len();
            chunk
        })
        .map_err(|_| ())
        .map(Ok);

    // Note: this closure simply creates a single future that sends all data then closes the sink.
    // It needs to be a single future because all 3 futures below need to make progress
    // concurrently for the upload to succeed (if the destinations aren't making progress, we'll
    // deadlock, if the source isn't making progress, we'll deadlock too, and if the sink doesn't
    // close, we'll never finish the uploads).
    let consume_stream = async {
        sink.send_all(&mut data)
            .await
            .map_err(|_| ErrorKind::ClientCancelled)
            .map_err(Error::from)?;

        sink.close().await?;

        Ok(())
    };

    let res = try_join!(internal_upload, upstream_upload, consume_stream);

    ScubaMiddlewareState::try_borrow_add(state, ScubaKey::RequestBytesReceived, received);

    res.map(|_| ())
}

async fn sync_internal_and_upstream(
    ctx: &RepositoryRequestContext,
    oid: Sha256,
    size: u64,
    state: &mut State,
) -> Result<(), Error> {
    let key = FetchKey::Aliased(Alias::Sha256(oid));

    let res = filestore::fetch(ctx.repo.blobstore(), ctx.ctx.clone(), &key)
        .compat()
        .await?;

    match res {
        Some(stream) => {
            // We have the data, so presumably upstream does not have it.
            ScubaMiddlewareState::try_borrow_add(
                state,
                ScubaKey::UploadSync,
                "internal_to_upstream",
            );
            upstream_upload(ctx, oid, size, stream.compat()).await?
        }
        None => {
            ScubaMiddlewareState::try_borrow_add(
                state,
                ScubaKey::UploadSync,
                "upstream_to_internal",
            );

            // We do not have the data. Get it from upstream.
            let object = RequestObject {
                oid: LfsSha256(oid.into_inner()),
                size,
            };

            let batch = RequestBatch {
                operation: Operation::Download,
                r#ref: None,
                transfers: vec![Transfer::Basic],
                objects: vec![object],
            };

            let batch = ctx
                .upstream_batch(&batch)
                .await
                .context(ErrorKind::UpstreamBatchError)?
                .ok_or_else(|| ErrorKind::ObjectCannotBeSynced(object))?;

            let action = find_actions(batch, &object)?
                .remove(&Operation::Download)
                .ok_or_else(|| ErrorKind::ObjectCannotBeSynced(object))?;

            let req = Request::get(action.href).body(Body::empty())?;

            let stream = ctx
                .dispatch(req)
                .await
                .context(ErrorKind::ObjectCannotBeSynced(object))?
                .into_inner();

            internal_upload(ctx, oid, size, stream).await?;
        }
    }

    Ok(())
}

fn read_content_length(state: &State) -> Option<Result<u64, Error>> {
    let headers = HeaderMap::try_borrow_from(&state)?;
    let val = headers.get(CONTENT_LENGTH)?;
    let size = str::from_utf8(val.as_bytes())
        .map_err(Error::from)
        .and_then(|val| val.parse().map_err(Error::from));
    Some(size)
}

pub async fn upload(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let UploadParams {
        repository,
        oid,
        size,
    } = state.take();

    let ctx =
        RepositoryRequestContext::instantiate(state, repository.clone(), LfsMethod::Upload).await?;

    let oid = Sha256::from_str(&oid).map_err(HttpError::e400)?;
    let size = size.parse().map_err(Error::from).map_err(HttpError::e400)?;
    let content_length = read_content_length(state)
        .transpose()
        .map_err(HttpError::e400)?;

    if let Some(content_length) = content_length {
        ScubaMiddlewareState::try_borrow_add(state, ScubaKey::RequestContentLength, content_length);
    }

    STATS::size_bytes.add_value(size as i64);

    if let Some(max_upload_size) = ctx.max_upload_size() {
        if size > max_upload_size {
            Err(HttpError::e400(ErrorKind::UploadTooLarge(
                size,
                max_upload_size,
            )))?;
        }
    }

    // The key invariant of our proxy design is that if you upload to this LFS server, then the
    // content will be present in both this LFS server and its upstream. To do so, we've
    // historically asked the client to upload whatever data either server is missing. However,
    // this only works if the client has the data, but it doesn't always. Indeed, if the client
    // only holds a LFS pointer (but knows the data is available on the server), then we can find
    // ourselves in a situation where the client needs to upload to us, but cannot (because it does
    // not have the data). However, the client still needs a mechanism to assert that the data is
    // in both this LFS server and its upstream. So, to support this mechanism, we let the client
    // send an empty request when uploading. When this happens, we assume we must have the content
    // somewhere, and try to sync it as necessary (to upstream if we have it internally, and to
    // internal if we don't).

    match content_length {
        Some(0) if size > 0 => {
            sync_internal_and_upstream(&ctx, oid, size, state)
                .await
                .map_err(HttpError::e500)?;
        }
        _ => {
            // TODO: More appropriate status codes here
            upload_from_client(&ctx, oid, size, state)
                .await
                .map_err(HttpError::e500)?;
        }
    }

    Ok(EmptyBody::new())
}

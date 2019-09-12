// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;
use failure::Error;
use futures::Future;
use futures_preview::{
    channel::mpsc::channel, compat::Future01CompatExt, compat::Stream01CompatExt, future::ready,
    SinkExt, Stream, StreamExt, TryStreamExt,
};
use futures_util::try_join;
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use hyper::{Body, Chunk, Request};
use serde::Deserialize;
use std::collections::HashMap;
use std::result::Result;
use std::str::FromStr;

use failure_ext::chain::ChainExt;
use filestore::StoreRequest;
use mononoke_types::hash::Sha256;

use crate::errors::ErrorKind;
use crate::http::{EmptyBody, HttpError, TryIntoResponse};
use crate::lfs_server_context::RequestContext;
use crate::protocol::{
    ObjectAction, ObjectStatus, Operation, RequestBatch, RequestObject, ResponseBatch, Transfer,
};

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

async fn discard_stream<S>(data: S) -> Result<(), Error>
where
    S: Stream<Item = Result<Bytes, Error>> + Unpin + Send + 'static,
{
    Ok(data.for_each(|_| ready(())).await)
}

async fn upstream_upload<S>(
    ctx: &RequestContext,
    oid: Sha256,
    size: u64,
    data: S,
) -> Result<(), Error>
where
    S: Stream<Item = Result<Bytes, Error>> + Unpin + Send + 'static,
{
    let object = RequestObject { oid, size };

    let batch = RequestBatch {
        operation: Operation::Upload,
        r#ref: None,
        transfers: vec![Transfer::Basic],
        objects: vec![object],
    };

    let res = ctx
        .upstream_batch(&batch)
        .await
        .chain_err(ErrorKind::UpstreamBatchError)?;

    let ResponseBatch { transfer, objects } = match res {
        Some(res) => res,
        None => {
            // We have no upstream: discard this copy.
            return discard_stream(data).await;
        }
    };

    let actions: Result<HashMap<Operation, ObjectAction>, Error> = match transfer {
        Transfer::Basic => objects
            .into_iter()
            .find(|o| o.object == object)
            .ok_or(ErrorKind::UpstreamMissingObject(object).into())
            .and_then(|o| match o.status {
                ObjectStatus::Ok {
                    authenticated: false,
                    actions,
                } => Ok(actions),
                _ => Err(ErrorKind::UpstreamInvalidObject(o).into()),
            }),
        Transfer::Unknown => Err(ErrorKind::UpstreamInvalidTransfer.into()),
    };

    if let Some(action) = actions?.remove(&Operation::Upload) {
        // TODO: We are discarding expiry and headers here. We probably shouldn't.
        // TODO: We are discarding verify actions.
        let ObjectAction { href, .. } = action;

        let body = Body::wrap_stream(data.compat());
        let req = Request::put(href)
            .header("Content-Length", &size.to_string())
            .body(body.into())?;

        // NOTE: We read the response body here, otherwise Hyper will not allow this connection to
        // be reused.
        let _ = ctx
            .dispatch(req)
            .await?
            .compat()
            .try_concat()
            .await
            .chain_err(ErrorKind::UpstreamUploadError)?;

        return Ok(());
    }

    discard_stream(data).await
}

pub async fn upload(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let UploadParams {
        repository,
        oid,
        size,
    } = state.take();

    let ctx = RequestContext::instantiate(state, repository.clone()).map_err(HttpError::e400)?;

    let oid = Sha256::from_str(&oid).map_err(HttpError::e400)?;
    let size = size.parse().map_err(Error::from).map_err(HttpError::e400)?;

    let (internal_send, internal_recv) = channel::<Result<Bytes, ()>>(BUFFER_SIZE);
    let (upstream_send, upstream_recv) = channel::<Result<Bytes, ()>>(BUFFER_SIZE);

    let mut sink = internal_send.fanout(upstream_send);

    let internal_recv = internal_recv
        .map_err(|()| ErrorKind::ClientCancelled)
        .err_into();

    let upstream_recv = upstream_recv
        .map_err(|()| ErrorKind::ClientCancelled)
        .err_into();

    let internal_upload = ctx
        .repo
        .upload_file(
            ctx.ctx.clone(),
            &StoreRequest::with_sha256(size, oid),
            internal_recv.compat(),
        )
        .chain_err(ErrorKind::FilestoreWriteFailure)
        .map_err(Error::from)
        .compat();

    let upstream_upload = upstream_upload(&ctx, oid, size, upstream_recv);

    let mut data = Body::take_from(state)
        .compat()
        .map_ok(Chunk::into_bytes)
        .map_err(|_| ());

    // Note: this closure simply creates a single future that sends all data then closes the sink.
    // It needs to be a single future because all 3 futures below need to make progress
    // concurrently for the upload to succeed (if the destinations aren't making progress, we'll
    // deadlock, if the source isn't making progress, we'll deadlock too, and if the sink doesn't
    // close, we'll never finish the uploads).
    let consume_stream = (async move || {
        sink.send_all(&mut data)
            .await
            .map_err(|_| ErrorKind::ClientCancelled)
            .map_err(Error::from)?;

        sink.close().await?;

        Ok(())
    })();

    try_join!(internal_upload, upstream_upload, consume_stream).map_err(HttpError::e500)?;

    Ok(EmptyBody::new())
}

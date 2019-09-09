// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;
use failure::Error;
use futures_preview::{
    channel::mpsc::channel, compat::Future01CompatExt, compat::Stream01CompatExt, future::ready,
    SinkExt, Stream, StreamExt, TryStreamExt,
};
use futures_util::try_join;
use gotham::{
    handler::IntoHandlerError,
    helpers::http::response::create_empty_response,
    state::{FromState, State},
};
use gotham_derive::{StateData, StaticResponseExtender};
use hyper::{Body, Chunk, Request, StatusCode};
use serde::Deserialize;
use std::collections::HashMap;
use std::result::Result;
use std::str::FromStr;

use failure_ext::chain::ChainExt;
use filestore::StoreRequest;
use mononoke_types::hash::Sha256;

use crate::errors::ErrorKind;
use crate::http::{git_lfs_mime, HandlerResponse};
use crate::lfs_server_context::RequestContext;
use crate::protocol::{
    ObjectAction, ObjectStatus, Operation, RequestBatch, RequestObject, ResponseBatch,
    ResponseError, Transfer,
};
use crate::{bail_http_400, bail_http_500};

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

// NOTE: This will need to be sent to a legacy Tokio executor, so it needs to take ownership of
// RequestContext.
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

    let ResponseBatch { transfer, objects } = ctx
        .upstream_batch(&batch)
        .await
        .chain_err(ErrorKind::UpstreamBatchError)?;

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

    let mut actions = actions?;

    if actions.contains_key(&Operation::Download) {
        // Upstream already has this object, so we just consume our stream and do nothing.
        return Ok(data.for_each(|_| ready(())).await);
    }

    if let Some(action) = actions.remove(&Operation::Upload) {
        // TODO: We are discarding expiry and headers here. We probably shouldn't.
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

    Err(ErrorKind::UpstreamBatchNoActions(object, actions).into())
}

pub async fn upload(mut state: State) -> HandlerResponse {
    let UploadParams {
        repository,
        oid,
        size,
    } = UploadParams::borrow_from(&state);

    let ctx = bail_http_400!(
        state,
        RequestContext::instantiate(&state, repository.clone())
    );
    let oid = bail_http_400!(state, Sha256::from_str(&oid));
    let size = bail_http_400!(state, size.parse().map_err(Error::from));

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
        .compat();

    let upstream_upload = upstream_upload(&ctx, oid, size, upstream_recv);

    let mut data = Body::take_from(&mut state)
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

    bail_http_500!(
        state,
        try_join!(internal_upload, upstream_upload, consume_stream)
    );

    let res = create_empty_response(&state, StatusCode::OK);
    Ok((state, res))
}

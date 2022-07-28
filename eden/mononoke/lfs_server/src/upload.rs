/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::str;
use std::str::FromStr;

use anyhow::Context;
use anyhow::Error;
use bytes::Bytes;
use filestore::Alias;
use filestore::FetchKey;
use filestore::FilestoreConfigRef;
use filestore::StoreRequest;
use futures::channel::mpsc::channel;
use futures::SinkExt;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use futures_util::try_join;
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
use hyper::Request;
use lfs_protocol::ObjectAction;
use lfs_protocol::ObjectStatus;
use lfs_protocol::Operation;
use lfs_protocol::RequestBatch;
use lfs_protocol::RequestObject;
use lfs_protocol::ResponseBatch;
use lfs_protocol::Sha256 as LfsSha256;
use lfs_protocol::Transfer;
use mononoke_types::hash::Sha256;
use repo_blobstore::RepoBlobstoreRef;
use serde::Deserialize;
use stats::prelude::*;

use crate::errors::ErrorKind;
use crate::lfs_server_context::RepositoryRequestContext;
use crate::middleware::LfsMethod;
use crate::scuba::LfsScubaKey;
use crate::util::read_header_value;

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

mod closeable_sender {
    use pin_project::pin_project;

    use futures::channel::mpsc::SendError;
    use futures::channel::mpsc::Sender;
    use futures::sink::Sink;
    use futures::task::Context;
    use futures::task::Poll;
    use std::pin::Pin;

    #[pin_project]
    pub struct CloseableSender<T> {
        #[pin]
        inner: Sender<T>,
    }

    impl<T> CloseableSender<T> {
        pub fn new(inner: Sender<T>) -> Self {
            Self { inner }
        }
    }

    impl<T> Sink<T> for CloseableSender<T> {
        type Error = SendError;

        fn poll_ready(
            self: Pin<&mut Self>,
            ctx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            match self.project().inner.poll_ready(ctx) {
                Poll::Ready(Err(e)) if e.is_disconnected() => Poll::Ready(Ok(())),
                x => x,
            }
        }

        fn start_send(self: Pin<&mut Self>, msg: T) -> Result<(), Self::Error> {
            match self.project().inner.start_send(msg) {
                Err(e) if e.is_disconnected() => Ok(()),
                x => x,
            }
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            ctx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            self.project().inner.poll_flush(ctx)
        }

        fn poll_close(
            self: Pin<&mut Self>,
            ctx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            self.project().inner.poll_close(ctx)
        }
    }
}

use closeable_sender::CloseableSender;

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
            .ok_or_else(|| ErrorKind::UpstreamMissingObject(*object).into())
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

    filestore::store(
        ctx.repo.repo_blobstore(),
        *ctx.repo.filestore_config(),
        &ctx.ctx,
        &StoreRequest::with_sha256(size, oid),
        data,
    )
    .await
    .context(ErrorKind::FilestoreWriteFailure)?;

    STATS::internal_success.add_value(1);

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
            return Ok(());
        }
    };

    let action = find_actions(batch, &object)?.remove(&Operation::Upload);

    if let Some(action) = action {
        // TODO: We are discarding expiry and headers here. We probably shouldn't.
        // TODO: We are discarding verify actions.
        STATS::upstream_uploads.add_value(1);
        let ObjectAction { href, .. } = action;

        let body = Body::wrap_stream(data);
        let req = Request::put(href)
            .header("Content-Length", &size.to_string())
            .body(body)?;

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

    Ok(())
}

async fn upload_from_client<S>(
    ctx: &RepositoryRequestContext,
    oid: Sha256,
    size: u64,
    body: S,
    scuba: &mut Option<&mut ScubaMiddlewareState>,
) -> Result<(), Error>
where
    S: Stream<Item = Result<Bytes, ()>> + Unpin + Send + 'static,
{
    let (internal_send, internal_recv) = channel::<Result<Bytes, ()>>(BUFFER_SIZE);
    let (upstream_send, upstream_recv) = channel::<Result<Bytes, ()>>(BUFFER_SIZE);

    // CloseableSender lets us allow a stream to close without breaking the sender. This is useful
    // if e.g. upstream already has the data we want to send, so we decide not to send it. This
    // gives us better error messages, since if one of our uploads fail, we're guaranteed that the
    // consume_stream future isn't the one that'll return an error.
    let internal_send = CloseableSender::new(internal_send);
    let upstream_send = CloseableSender::new(upstream_send);

    let mut sink = internal_send.fanout(upstream_send);

    let internal_recv = internal_recv
        .map_err(|()| ErrorKind::ClientCancelled)
        .err_into();

    let upstream_recv = upstream_recv
        .map_err(|()| ErrorKind::ClientCancelled)
        .err_into();

    let internal_upload = internal_upload(ctx, oid, size, internal_recv);
    let upstream_upload = upstream_upload(ctx, oid, size, upstream_recv);

    let mut received: usize = 0;

    let mut data = body
        .map_ok(|chunk| {
            received += chunk.len();
            chunk
        })
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

    ScubaMiddlewareState::maybe_add(scuba, HttpScubaKey::RequestBytesReceived, received);

    res.map(|_| ())
}

async fn sync_internal_and_upstream(
    ctx: &RepositoryRequestContext,
    oid: Sha256,
    size: u64,
    scuba: &mut Option<&mut ScubaMiddlewareState>,
) -> Result<(), Error> {
    let key = FetchKey::Aliased(Alias::Sha256(oid));

    let res = filestore::fetch(ctx.repo.repo_blobstore().clone(), ctx.ctx.clone(), &key).await?;

    match res {
        Some(stream) => {
            // We have the data, so presumably upstream does not have it.
            ScubaMiddlewareState::maybe_add(scuba, LfsScubaKey::UploadSync, "internal_to_upstream");
            upstream_upload(ctx, oid, size, stream).await?;
        }
        None => {
            // We do not have the data. Get it from upstream.
            ScubaMiddlewareState::maybe_add(scuba, LfsScubaKey::UploadSync, "upstream_to_internal");
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
                .ok_or(ErrorKind::ObjectCannotBeSynced(object))?;

            let action = find_actions(batch, &object)?
                .remove(&Operation::Download)
                .ok_or(ErrorKind::ObjectCannotBeSynced(object))?;

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
            let mut scuba = state.try_borrow_mut::<ScubaMiddlewareState>();
            sync_internal_and_upstream(&ctx, oid, size, &mut scuba)
                .await
                .map_err(HttpError::e500)?;
        }
        _ => {
            // TODO: More appropriate status codes here
            let body = Body::take_from(state).map_err(|_| ());
            let mut scuba = state.try_borrow_mut::<ScubaMiddlewareState>();
            upload_from_client(&ctx, oid, size, body, &mut scuba)
                .await
                .map_err(HttpError::e500)?;
        }
    }

    Ok(EmptyBody::new())
}

#[cfg(test)]
mod test {
    use super::*;
    use chaosblob::ChaosBlobstore;
    use chaosblob::ChaosOptions;
    use fbinit::FacebookInit;
    use futures::future;
    use futures::stream;
    use memblob::Memblob;
    use std::num::NonZeroU32;
    use std::sync::Arc;
    use test_repo_factory::TestRepoFactory;

    #[fbinit::test]
    async fn test_upload_from_client_discard_upstream(fb: FacebookInit) -> Result<(), Error> {
        let ctx = RepositoryRequestContext::test_builder(fb)?
            .upstream_uri(None)
            .build()?;

        let body = stream::once(future::ready(Ok(Bytes::from("foobar"))));
        let oid =
            Sha256::from_str("c3ab8ff13720e8ad9047dd39466b3c8974e592c2fa383d4a3960714caef0c4f2")?;
        let size = 6;

        upload_from_client(&ctx, oid, size, body, &mut None).await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_upload_from_client_failing_internal(fb: FacebookInit) -> Result<(), Error> {
        // Create a test repo with a blobstore that fails all reads and writes.
        let repo = TestRepoFactory::new(fb)?
            .with_blobstore(Arc::new(ChaosBlobstore::new(
                Memblob::default(),
                ChaosOptions::new(NonZeroU32::new(1), NonZeroU32::new(1)),
            )))
            .build()?;

        let ctx = RepositoryRequestContext::test_builder_with_repo(fb, repo)?
            .upstream_uri(None)
            .build()?;

        let body = stream::once(future::ready(Ok(Bytes::from("foobar"))));
        let oid =
            Sha256::from_str("c3ab8ff13720e8ad9047dd39466b3c8974e592c2fa383d4a3960714caef0c4f2")?;
        let size = 6;

        let r = upload_from_client(&ctx, oid, size, body, &mut None).await;

        assert!(r.is_err());

        Ok(())
    }
}

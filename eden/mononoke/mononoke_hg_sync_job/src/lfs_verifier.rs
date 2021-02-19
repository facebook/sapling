/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::{anyhow, Context, Error};
use blobstore::Blobstore;
use bytes_old::Bytes as BytesOld;
use cloned::cloned;
use context::CoreContext;
use failure_ext::FutureFailureExt;
use filestore::{fetch_stream, FetchKey};
use futures::{
    compat::Future01CompatExt, pin_mut, FutureExt, StreamExt, TryFutureExt, TryStreamExt,
};
use futures_01_ext::{try_boxfuture, FutureExt as OldFutureExt};
use futures_old::{future, stream, Future, IntoFuture, Stream};
use http::{status::StatusCode, uri::Uri};
use hyper::{client::HttpConnector, Client};
use hyper::{Body, Request};
use hyper_openssl::HttpsConnector;
use slog::{info, warn};
use thiserror::Error;

use lfs_protocol::{
    ObjectAction, ObjectStatus, Operation, RequestBatch, RequestObject, ResponseBatch,
    ResponseObject, Sha256 as LfsSha256, Transfer,
};
use mononoke_types::hash::Sha256;

pub type HttpsHyperClient = Client<HttpsConnector<HttpConnector>>;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Serializing a LFS batch failed")]
    SerializationFailed,
    #[error("Deserializating a LFS batch failed")]
    DeserializationFailed,
    #[error("Creating a request failed")]
    RequestCreationFailed,
    #[error("Submitting a batch request failed")]
    BatchRequestNoResponse,
    #[error("Submitting a batch request failed with status {0}")]
    BatchRequestFailed(StatusCode),
    #[error("Reading the response for a batch request failed")]
    BatchRequestReadFailed,
    #[error("An error ocurred receiving a response from upstream ({0}): {1}")]
    UpstreamError(StatusCode, String),
}

struct LfsVerifierInner {
    client: HttpsHyperClient,
    batch_uri: Uri,
    blobstore: Arc<dyn Blobstore>,
}

#[derive(Clone)]
pub struct LfsVerifier {
    inner: Arc<LfsVerifierInner>,
}

impl LfsVerifier {
    pub fn new(batch_uri: Uri, blobstore: Arc<dyn Blobstore>) -> Result<Self, Error> {
        let connector = HttpsConnector::new(4)?;
        let client = Client::builder().build(connector);

        let inner = LfsVerifierInner {
            batch_uri,
            client,
            blobstore,
        };

        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    pub fn ensure_lfs_presence(
        &self,
        ctx: CoreContext,
        blobs: &[(Sha256, u64)],
    ) -> impl Future<Item = (), Error = Error> {
        let batch = build_upload_request_batch(blobs);
        let body: BytesOld =
            try_boxfuture!(serde_json::to_vec(&batch).context(ErrorKind::SerializationFailed))
                .into();

        let uri = self.inner.batch_uri.clone();
        let req = try_boxfuture!(
            Request::post(uri)
                .body(body.into())
                .context(ErrorKind::RequestCreationFailed)
        );

        let blobstore = self.inner.blobstore.clone();

        let client = self.inner.client.clone();
        self.inner
            .client
            .request(req)
            .context(ErrorKind::BatchRequestNoResponse)
            .map_err(Error::from)
            .and_then(|response| {
                let (head, body) = response.into_parts();

                if !head.status.is_success() {
                    return Err(ErrorKind::BatchRequestFailed(head.status).into())
                        .into_future()
                        .left_future();
                }

                body.concat2()
                    .context(ErrorKind::BatchRequestReadFailed)
                    .map_err(Error::from)
                    .right_future()
            })
            .and_then(|body| {
                serde_json::from_slice::<ResponseBatch>(&body)
                    .context(ErrorKind::DeserializationFailed)
                    .map_err(Error::from)
            })
            .and_then(move |batch| {
                let missing_objects = find_missing_objects(batch);

                if missing_objects.is_empty() {
                    return future::ok(()).boxify();
                }

                for object in &missing_objects {
                    warn!(ctx.logger(), "missing {:?} object, uploading", object);
                }

                stream::iter_ok(missing_objects)
                    .map(move |object| {
                        cloned!(ctx, client, object, blobstore);
                        async move { upload(ctx, client, object, blobstore).await }
                            .boxed()
                            .compat()
                    })
                    .buffer_unordered(100)
                    .for_each(|_| Ok(()))
                    .boxify()
            })
            .boxify()
    }
}

fn build_upload_request_batch(blobs: &[(Sha256, u64)]) -> RequestBatch {
    let objects = blobs
        .iter()
        .map(|(oid, size)| RequestObject {
            oid: LfsSha256(oid.into_inner()),
            size: *size,
        })
        .collect();

    RequestBatch {
        operation: Operation::Upload,
        r#ref: None,
        transfers: vec![Transfer::Basic],
        objects,
    }
}

fn find_missing_objects(batch: ResponseBatch) -> Vec<ResponseObject> {
    batch
        .objects
        .into_iter()
        .filter_map(|object| match object.status {
            ObjectStatus::Ok { ref actions, .. } if !actions.contains_key(&Operation::Upload) => {
                None
            }
            _ => Some(object),
        })
        .collect()
}

async fn upload(
    ctx: CoreContext,
    client: HttpsHyperClient,
    resp_object: ResponseObject,
    blobstore: Arc<dyn Blobstore>,
) -> Result<(), Error> {
    match resp_object.status {
        ObjectStatus::Ok { actions, .. } => match actions.get(&Operation::Upload) {
            Some(action) => {
                let ObjectAction { href, .. } = action;

                let key = FetchKey::from(Sha256::from_byte_array(resp_object.object.oid.0));
                let s = ({
                    cloned!(ctx);
                    async_stream::stream! {
                        let s = fetch_stream(
                            &blobstore,
                            ctx.clone(),
                            key,
                        );

                        pin_mut!(s);
                        while let Some(value) = s.next().await {
                            yield value;
                        }
                    }
                })
                .boxed()
                .compat();

                let body = Body::wrap_stream(s.map(|s| s.to_vec()));
                let req = Request::put(format!("{}", href))
                    .header("Content-Length", &resp_object.object.size.to_string())
                    .body(body)?;

                let res = client.request(req).compat().await?;
                let (head, body) = res.into_parts();

                if !head.status.is_success() {
                    let body = body.concat2().compat().await?;
                    return Err(ErrorKind::UpstreamError(
                        head.status,
                        String::from_utf8_lossy(&body).to_string(),
                    )
                    .into());
                } else {
                    info!(
                        ctx.logger(),
                        "uploaded content for {:?}", resp_object.object
                    );
                }

                Ok(())
            }
            None => Err(anyhow!(
                "not found upload action for {:?}",
                resp_object.object
            )),
        },
        ObjectStatus::Err { error } => Err(anyhow!(
            "batch failed for {:?} {:?}",
            resp_object.object,
            error
        )),
    }
}

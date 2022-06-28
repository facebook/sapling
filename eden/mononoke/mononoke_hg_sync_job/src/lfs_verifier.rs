/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use blobstore::Blobstore;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use filestore::fetch_stream;
use filestore::FetchKey;
use futures::future;
use futures::pin_mut;
use futures::stream;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures_old::Future;
use gotham_ext::body_ext::BodyExt;
use http::status::StatusCode;
use http::uri::Uri;
use hyper::client::HttpConnector;
use hyper::Body;
use hyper::Client;
use hyper::Request;
use hyper_openssl::HttpsConnector;
use slog::info;
use slog::warn;
use thiserror::Error;

use lfs_protocol::ObjectAction;
use lfs_protocol::ObjectStatus;
use lfs_protocol::Operation;
use lfs_protocol::RequestBatch;
use lfs_protocol::RequestObject;
use lfs_protocol::ResponseBatch;
use lfs_protocol::ResponseObject;
use lfs_protocol::Sha256 as LfsSha256;
use lfs_protocol::Transfer;
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
        let connector = HttpsConnector::new()?;
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
        let uri = self.inner.batch_uri.clone();
        let blobstore = self.inner.blobstore.clone();
        let client = self.inner.client.clone();

        let batch = build_upload_request_batch(blobs);

        async move {
            let body =
                Bytes::from(serde_json::to_vec(&batch).context(ErrorKind::SerializationFailed)?);

            let req = Request::post(uri)
                .body(body.into())
                .context(ErrorKind::RequestCreationFailed)?;

            let response = client
                .request(req)
                .await
                .context(ErrorKind::BatchRequestNoResponse)?;

            let (head, body) = response.into_parts();

            if !head.status.is_success() {
                return Result::<_, Error>::Err(ErrorKind::BatchRequestFailed(head.status).into())?;
            }

            let body = body
                .try_concat_body(&head.headers)
                .context(ErrorKind::BatchRequestReadFailed)?
                .await
                .context(ErrorKind::BatchRequestReadFailed)?;

            let batch = serde_json::from_slice::<ResponseBatch>(&body)
                .context(ErrorKind::DeserializationFailed)?;

            let missing_objects = find_missing_objects(batch);

            if missing_objects.is_empty() {
                return Ok(());
            }

            for object in &missing_objects {
                warn!(ctx.logger(), "missing {:?} object, uploading", object);
            }

            stream::iter(missing_objects)
                .map(|object| upload(&ctx, &client, object, blobstore.clone()))
                .buffer_unordered(100)
                .try_for_each(|()| future::ready(Ok(())))
                .await?;

            Result::<_, Error>::Ok(())
        }
        .boxed()
        .compat()
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

async fn upload<'a>(
    ctx: &'a CoreContext,
    client: &'a HttpsHyperClient,
    resp_object: ResponseObject,
    blobstore: Arc<dyn Blobstore>,
) -> Result<(), Error> {
    match resp_object.status {
        ObjectStatus::Ok { actions, .. } => match actions.get(&Operation::Upload) {
            Some(action) => {
                let ObjectAction { href, .. } = action;

                let key = FetchKey::from(Sha256::from_byte_array(resp_object.object.oid.0));

                let s = {
                    cloned!(ctx);
                    async_stream::stream! {
                        let s = fetch_stream(
                            &blobstore,
                            ctx,
                            key,
                        );

                        pin_mut!(s);
                        while let Some(value) = s.next().await {
                            yield value;
                        }
                    }
                };

                let body = Body::wrap_stream(s.map_ok(|b| Bytes::copy_from_slice(b.as_ref())));

                let req = Request::put(format!("{}", href))
                    .header("Content-Length", &resp_object.object.size.to_string())
                    .body(body)?;

                let res = client.request(req).await?;
                let (head, body) = res.into_parts();

                let body = body.try_concat_body(&head.headers)?.await?;

                if !head.status.is_success() {
                    return Err(ErrorKind::UpstreamError(
                        head.status,
                        String::from_utf8_lossy(&body).to_string(),
                    )
                    .into());
                }

                info!(
                    ctx.logger(),
                    "uploaded content for {:?}", resp_object.object
                );

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

/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::{Context, Error};
use bytes_old::Bytes as BytesOld;
use failure_ext::FutureFailureExt;
use futures_ext::{try_boxfuture, FutureExt};
use futures_old::{Future, IntoFuture, Stream};
use http::{status::StatusCode, uri::Uri};
use hyper::Request;
use hyper::{client::HttpConnector, Client};
use hyper_openssl::HttpsConnector;
use thiserror::Error;

use lfs_protocol::{
    ObjectStatus, Operation, RequestBatch, RequestObject, ResponseBatch, Sha256 as LfsSha256,
    Transfer,
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
    #[error("LFS objects are missing: {0:?}")]
    LfsObjectsMissing(Vec<RequestObject>),
}

struct LfsVerifierInner {
    client: HttpsHyperClient,
    batch_uri: Uri,
}

#[derive(Clone)]
pub struct LfsVerifier {
    inner: Arc<LfsVerifierInner>,
}

impl LfsVerifier {
    pub fn new(batch_uri: Uri) -> Result<Self, Error> {
        let connector = HttpsConnector::new(4)?;
        let client = Client::builder().build(connector);

        let inner = LfsVerifierInner { batch_uri, client };

        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    pub fn verify_lfs_presence(
        &self,
        blobs: &[(Sha256, u64)],
    ) -> impl Future<Item = (), Error = Error> {
        let batch = build_download_request_batch(blobs);
        let body: BytesOld =
            try_boxfuture!(serde_json::to_vec(&batch).context(ErrorKind::SerializationFailed))
                .into();

        let uri = self.inner.batch_uri.clone();
        let req = try_boxfuture!(Request::post(uri)
            .body(body.into())
            .context(ErrorKind::RequestCreationFailed));

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
            .and_then(|batch| {
                let missing_objects = find_missing_objects(batch);

                if missing_objects.is_empty() {
                    return Ok(());
                }

                Err(ErrorKind::LfsObjectsMissing(missing_objects).into())
            })
            .boxify()
    }
}

fn build_download_request_batch(blobs: &[(Sha256, u64)]) -> RequestBatch {
    let objects = blobs
        .iter()
        .map(|(oid, size)| RequestObject {
            oid: LfsSha256(oid.into_inner()),
            size: *size,
        })
        .collect();

    RequestBatch {
        operation: Operation::Download,
        r#ref: None,
        transfers: vec![Transfer::Basic],
        objects,
    }
}

fn find_missing_objects(batch: ResponseBatch) -> Vec<RequestObject> {
    batch
        .objects
        .into_iter()
        .filter_map(|object| match object.status {
            ObjectStatus::Ok { ref actions, .. } if actions.contains_key(&Operation::Download) => {
                None
            }
            _ => Some(object.object),
        })
        .collect()
}

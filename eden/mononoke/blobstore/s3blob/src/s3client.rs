/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreMetadata;
use chrono::DateTime;
use context::CoreContext;
use context::PerfCounterType;
use futures_stats::futures03::TimedTryFutureExt;
use hyper::client::HttpConnector;
use hyper::StatusCode;
use openssl::ssl::SslConnector;
use openssl::ssl::SslMethod;
use rusoto_core::HttpClient;
use rusoto_core::Region;
use rusoto_core::RusotoError;
use rusoto_credential::ProvideAwsCredentials;
use rusoto_s3::DeleteObjectRequest;
use rusoto_s3::GetObjectError;
use rusoto_s3::GetObjectRequest;
use rusoto_s3::HeadObjectError;
use rusoto_s3::HeadObjectRequest;
use rusoto_s3::PutObjectRequest;
use rusoto_s3::S3Client;
use rusoto_s3::S3;
use time_ext::DurationExt;
use tokio::io::AsyncReadExt;
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;

pub fn get_s3_client<P: ProvideAwsCredentials + Send + Sync + 'static>(
    credential_provider: P,
    region: Region,
    semaphore: Option<Arc<Semaphore>>,
) -> Result<S3ClientWrapper> {
    let http_client = get_http_client()?;
    Ok(S3ClientWrapper {
        client: S3Client::new_with(http_client, credential_provider, region),
        semaphore,
    })
}

fn get_http_client() -> Result<HttpClient<hyper_openssl::HttpsConnector<HttpConnector>>, Error> {
    let mut http_connector = HttpConnector::new();
    http_connector.enforce_http(false);
    http_connector.set_keepalive(Some(Duration::from_secs(1)));
    let ssl_connector = SslConnector::builder(SslMethod::tls())?;
    let https_connector =
        hyper_openssl::HttpsConnector::with_connector(http_connector, ssl_connector)?;

    Ok(HttpClient::from_connector(https_connector))
}

pub struct S3ClientWrapper {
    client: S3Client,
    semaphore: Option<Arc<Semaphore>>,
}

impl S3ClientWrapper {
    pub async fn get(
        self: Arc<Self>,
        ctx: &CoreContext,
        request: GetObjectRequest,
    ) -> Result<Option<BlobstoreGetData>> {
        let ctx = ctx.clone();
        let f = async move {
            // Keep it alive for the duration of the operation
            let _permit = self.get_permit(&ctx).await?;

            let obj = match self.client.get_object(request).await {
                Ok(obj) => obj,
                Err(RusotoError::Service(GetObjectError::NoSuchKey(_))) => {
                    return Ok(None);
                }
                Err(RusotoError::Unknown(http_response)) => {
                    return Err(anyhow!(format!(
                        "status: {}; Error: {}",
                        http_response.status,
                        http_response.body_as_str()
                    )))
                    .with_context(|| "while fetching blob from S3");
                }
                Err(e) => {
                    return Err(Error::from(e)).with_context(|| "while fetching blob from S3");
                }
            };
            if let Some(stream) = obj.body {
                let content_length = obj
                    .content_length
                    .ok_or_else(|| anyhow!("Object response doesn't have content length field"))?
                    .try_into()?;
                let mut body = Vec::with_capacity(content_length);
                stream.into_async_read().read_to_end(&mut body).await?;
                if body.len() != content_length {
                    return Err(anyhow!(format!(
                        "Couldn't fetch the object from S3 storage. \
                Expected {} bytes, received {}",
                        body.len(),
                        content_length
                    )));
                }
                let ctime = obj.last_modified.and_then(|t| {
                    DateTime::parse_from_rfc2822(t.as_str())
                        .map(|time| time.timestamp())
                        .ok()
                });
                Ok(Some(BlobstoreGetData::new(
                    BlobstoreMetadata::new(ctime, None),
                    BlobstoreBytes::from_bytes(body),
                )))
            } else {
                Ok(None)
            }
        };

        // NOTE - this spawn is intentional! Removing it can cause a deadlock. See D27235628
        tokio::spawn(f).await?
    }

    pub async fn is_present(
        self: Arc<Self>,
        ctx: &CoreContext,
        request: HeadObjectRequest,
    ) -> Result<bool> {
        let ctx = ctx.clone();
        let f = async move {
            // Keep it alive for the duration of the operation
            let _permit = self.get_permit(&ctx).await?;

            match self.client.head_object(request).await {
                Ok(_) => Ok(true),
                Err(RusotoError::Service(HeadObjectError::NoSuchKey(_))) => Ok(false),
                // Currently we are not getting HeadObjectError::NoSuchKey(_) when the key is missing
                // instead we receive HTTP response with 404 status code.
                Err(RusotoError::Unknown(http_response))
                    if StatusCode::NOT_FOUND == http_response.status =>
                {
                    Ok(false)
                }
                Err(RusotoError::Unknown(http_response)) => Err(anyhow!(format!(
                    "status: {}; Error: {}",
                    http_response.status,
                    http_response.body_as_str()
                )))
                .with_context(|| "while fetching blob from S3"),
                Err(e) => Err(Error::from(e)).with_context(|| "while fetching blob from S3"),
            }
        };

        // NOTE - this spawn is intentional! Removing it can cause a deadlock. See D27235628
        tokio::spawn(f).await?
    }

    pub async fn put(
        self: Arc<Self>,
        ctx: &CoreContext,
        request: PutObjectRequest,
    ) -> Result<(), Error> {
        let ctx = ctx.clone();

        let f = async move {
            // Keep it alive for the duration of the operation
            let _permit = self.get_permit(&ctx).await?;

            self.client
                .put_object(request)
                .await
                .with_context(|| "While writing blob into S3 storage")?;
            Ok(())
        };

        // NOTE - this spawn is intentional! Removing it can cause a deadlock. See D27235628
        tokio::spawn(f).await?
    }

    pub async fn unlink(
        self: Arc<Self>,
        ctx: &CoreContext,
        request: DeleteObjectRequest,
    ) -> Result<(), Error> {
        let ctx = ctx.clone();

        let f = async move {
            // Keep it alive for the duration of the operation
            let _permit = self.get_permit(&ctx).await?;

            self.client
                .delete_object(request)
                .await
                .with_context(|| "While deleting blob from S3 storage")?;
            Ok(())
        };

        // NOTE - this spawn is intentional! Removing it can cause a deadlock. See D27235628
        tokio::spawn(f).await?
    }

    async fn get_permit(&self, ctx: &CoreContext) -> Result<Option<SemaphorePermit<'_>>, Error> {
        let ret = match self.semaphore {
            Some(ref sem) => {
                let (stats, permit) = sem.acquire().try_timed().await?;

                ctx.perf_counters().add_to_counter(
                    PerfCounterType::S3AccessWait,
                    stats.completion_time.as_millis_unchecked() as i64,
                );

                Some(permit)
            }
            None => None,
        };

        Ok(ret)
    }
}

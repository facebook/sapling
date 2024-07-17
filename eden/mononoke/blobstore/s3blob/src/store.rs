/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::future::Future;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use anyhow::format_err;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstorePutOps;
use blobstore::BlobstoreUnlinkOps;
use blobstore::CountedBlobstore;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use context::CoreContext;
use context::PerfCounterType;
use mononoke_types::BlobstoreBytes;
use rusoto_s3::DeleteObjectRequest;
use rusoto_s3::GetObjectRequest;
use rusoto_s3::HeadObjectRequest;
use rusoto_s3::PutObjectRequest;

use crate::S3ClientBackend;

const MAX_ATTEMPT_NUM: NonZeroU32 = nonzero_ext::nonzero!(4u32);

async fn retry<Retryable, V, Fut>(ctx: &CoreContext, retryable: Retryable) -> Result<V>
where
    Retryable: Fn() -> Fut,
    V: Send + Sync + 'static,
    Fut: Future<Output = Result<V>>,
{
    let mut attempt = nonzero_ext::nonzero!(1u32);

    loop {
        let resp = retryable().await;

        match resp {
            Ok(v) => return Ok(v),
            Err(e) => {
                let duration = if attempt >= MAX_ATTEMPT_NUM {
                    None
                } else {
                    Some(Duration::from_millis(100 * 4u64.pow(attempt.get() - 1)))
                };
                if let Some(duration) = duration {
                    let pc = ctx.perf_counters();
                    pc.increment_counter(PerfCounterType::S3BlobRetries);
                    pc.add_to_counter(PerfCounterType::S3BlobSumDelay, duration.as_millis() as i64);
                    tokio::time::sleep(duration).await;
                    attempt = NonZeroU32::new(attempt.get() + 1).unwrap();
                    continue;
                } else {
                    return Err(e.context(format_err!("Request failed on attempt {}", attempt)));
                }
            }
        }
    }
}

pub struct S3Blob<ClientBackend: S3ClientBackend + Send + Sync> {
    bucket: String,
    client_pool: Arc<ClientBackend>,
    put_behaviour: PutBehaviour,
}

impl<ClientBackend: S3ClientBackend + Send + Sync> fmt::Display for S3Blob<ClientBackend> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "S3Blob")
    }
}

impl<ClientBackend: S3ClientBackend + Send + Sync> fmt::Debug for S3Blob<ClientBackend> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("S3Blob")
            .field("bucket", &self.bucket)
            .field("client", &"S3Client")
            .finish()
    }
}

impl<ClientBackend: S3ClientBackend + Send + Sync> S3Blob<ClientBackend> {
    pub async fn new<T: ToString>(
        bucket: T,
        client_backend: Arc<ClientBackend>,
        put_behaviour: PutBehaviour,
    ) -> Result<CountedBlobstore<Self>> {
        let blob = Self {
            bucket: bucket.to_string(),
            client_pool: client_backend,
            put_behaviour,
        };
        Ok(CountedBlobstore::new(
            format!("s3.{}", bucket.to_string()),
            blob,
        ))
    }
}

#[async_trait]
impl<ClientBackend: S3ClientBackend + Send + Sync> Blobstore for S3Blob<ClientBackend> {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        retry(ctx, || {
            let key = ClientBackend::get_sharded_key(key);
            let request = GetObjectRequest {
                bucket: self.bucket.to_string(),
                key,
                ..Default::default()
            };

            async move { self.client_pool.get_client().get(ctx, request).await }
        })
        .await
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        retry(ctx, || {
            let key = ClientBackend::get_sharded_key(key);
            let request = HeadObjectRequest {
                bucket: self.bucket.to_string(),
                key,
                ..Default::default()
            };
            async move {
                let present = self
                    .client_pool
                    .get_client()
                    .is_present(ctx, request)
                    .await?;
                Ok(if present {
                    BlobstoreIsPresent::Present
                } else {
                    BlobstoreIsPresent::Absent
                })
            }
        })
        .await
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        BlobstorePutOps::put_with_status(self, ctx, key, value).await?;
        Ok(())
    }
}

#[async_trait]
impl<ClientBackend: S3ClientBackend + Send + Sync> BlobstorePutOps for S3Blob<ClientBackend> {
    async fn put_explicit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        orig_key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        retry(ctx, || {
            let key = ClientBackend::get_sharded_key(&orig_key);
            let content: Vec<u8> = value.as_bytes().as_ref().to_vec();
            let request = PutObjectRequest {
                bucket: self.bucket.to_string(),
                key,
                body: Some(content.into()),
                ..Default::default()
            };
            let orig_key = &orig_key;
            async move {
                let put_fut = async {
                    self.client_pool.get_client().put(ctx, request).await?;
                    Ok(OverwriteStatus::NotChecked)
                };

                match put_behaviour {
                    PutBehaviour::Overwrite => put_fut.await,
                    PutBehaviour::IfAbsent | PutBehaviour::OverwriteAndLog => {
                        if self.is_present(ctx, orig_key).await?.fail_if_unsure()? {
                            if put_behaviour.should_overwrite() {
                                put_fut.await?;
                                Ok(OverwriteStatus::Overwrote)
                            } else {
                                // discard the put
                                std::mem::drop(put_fut);
                                Ok(OverwriteStatus::Prevented)
                            }
                        } else {
                            put_fut.await?;
                            Ok(OverwriteStatus::New)
                        }
                    }
                }
            }
        })
        .await
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.put_explicit(ctx, key, value, self.put_behaviour).await
    }
}

#[async_trait]
impl<ClientBackend: S3ClientBackend + Send + Sync> BlobstoreUnlinkOps for S3Blob<ClientBackend> {
    async fn unlink<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        retry(ctx, || {
            let key = ClientBackend::get_sharded_key(key);
            let request = DeleteObjectRequest {
                bucket: self.bucket.to_string(),
                key,
                ..Default::default()
            };

            async move { self.client_pool.get_client().unlink(ctx, request).await }
        })
        .await
    }
}

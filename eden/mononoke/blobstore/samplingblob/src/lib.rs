/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Result;
use async_trait::async_trait;
use blobstore::{Blobstore, BlobstoreGetData};
use context::CoreContext;
use mononoke_types::BlobstoreBytes;
use std::sync::Arc;

pub trait SamplingHandler: std::fmt::Debug + Send + Sync {
    fn sample_get(
        &self,
        ctx: &CoreContext,
        key: &str,
        value: Option<&BlobstoreBytes>,
    ) -> Result<()>;

    fn sample_put(&self, _ctx: &CoreContext, _key: &str, _value: &BlobstoreBytes) -> Result<()> {
        Ok(())
    }

    fn sample_is_present(&self, _ctx: &CoreContext, _key: &str, _value: bool) -> Result<()> {
        Ok(())
    }
}

/// A layer over an existing blobstore that allows sampling of blobs, e.g. for
/// corpus generation.
#[derive(Debug)]
pub struct SamplingBlobstore<T> {
    inner: T,
    handler: Arc<dyn SamplingHandler>,
}

impl<T> SamplingBlobstore<T> {
    pub fn new(inner: T, handler: Arc<dyn SamplingHandler>) -> Self {
        Self { inner, handler }
    }
}

#[async_trait]
impl<T: Blobstore> Blobstore for SamplingBlobstore<T> {
    #[inline]
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let opt_blob = self.inner.get(ctx, key).await?;
        self.handler
            .sample_get(ctx, key, opt_blob.as_ref().map(|blob| blob.as_bytes()))?;
        Ok(opt_blob)
    }

    #[inline]
    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        let sample_res = self.handler.sample_put(&ctx, &key, &value);
        self.inner.put(ctx, key, value).await?;
        sample_res
    }

    #[inline]
    async fn is_present<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<bool> {
        let is_present = self.inner.is_present(ctx, key).await?;
        self.handler.sample_is_present(ctx, key, is_present)?;
        Ok(is_present)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use borrowed::borrowed;
    use fbinit::FacebookInit;
    use std::sync::atomic::{AtomicBool, Ordering};

    use context::SamplingKey;
    use memblob::Memblob;

    #[derive(Debug)]
    struct TestSamplingHandler {
        sampled: AtomicBool,
        looking_for: SamplingKey,
    }
    impl TestSamplingHandler {
        fn check_sample(&self, ctx: &CoreContext) -> Result<()> {
            ctx.sampling_key().map(|sampling_key| {
                if sampling_key == &self.looking_for {
                    self.sampled.store(true, Ordering::Relaxed);
                }
            });
            Ok(())
        }
    }

    impl SamplingHandler for TestSamplingHandler {
        fn sample_get(
            &self,
            ctx: &CoreContext,
            _key: &str,
            _value: Option<&BlobstoreBytes>,
        ) -> Result<()> {
            self.check_sample(ctx)
        }
        fn sample_put(&self, ctx: &CoreContext, _key: &str, _value: &BlobstoreBytes) -> Result<()> {
            self.check_sample(ctx)
        }
        fn sample_is_present(&self, ctx: &CoreContext, _key: &str, _value: bool) -> Result<()> {
            self.check_sample(ctx)
        }
    }

    #[fbinit::test]
    async fn test_sample_called(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);
        let base = Memblob::default();
        let sample_this = SamplingKey::new();
        let handler = Arc::new(TestSamplingHandler {
            sampled: AtomicBool::new(false),
            looking_for: sample_this,
        });
        let wrapper =
            SamplingBlobstore::new(base.clone(), handler.clone() as Arc<dyn SamplingHandler>);
        let key = "foobar";

        let r = wrapper
            .put(
                ctx,
                key.to_owned(),
                BlobstoreBytes::from_bytes("test foobar"),
            )
            .await;
        assert!(r.is_ok());
        let was_sampled = handler.sampled.load(Ordering::Relaxed);
        assert!(!was_sampled);
        let ctx = ctx.clone_and_sample(sample_this);
        borrowed!(ctx);
        let base_present = base.is_present(ctx, key).await.unwrap();
        assert!(base_present);
        let was_sampled = handler.sampled.load(Ordering::Relaxed);
        assert!(!was_sampled);
        let wrapper_present = wrapper.is_present(ctx, key).await.unwrap();
        assert!(wrapper_present);
        let was_sampled = handler.sampled.load(Ordering::Relaxed);
        assert!(was_sampled);
    }
}

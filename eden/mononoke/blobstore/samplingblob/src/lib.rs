/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstorePutOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use context::CoreContext;
use metaconfig_types::BlobstoreId;
use mononoke_types::BlobstoreBytes;
use std::sync::Arc;

pub trait SamplingHandler: std::fmt::Debug + Send + Sync {
    fn sample_get(
        &self,
        ctx: &CoreContext,
        key: &str,
        value: Option<&BlobstoreGetData>,
    ) -> Result<()>;

    fn sample_put(&self, _ctx: &CoreContext, _key: &str, _value: &BlobstoreBytes) -> Result<()> {
        Ok(())
    }

    fn sample_is_present(
        &self,
        _ctx: &CoreContext,
        _key: &str,
        _value: &BlobstoreIsPresent,
    ) -> Result<()> {
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

impl<T: std::fmt::Display> std::fmt::Display for SamplingBlobstore<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "SamplingBlobstore<{}>", &self.inner)
    }
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
        self.handler.sample_get(ctx, key, opt_blob.as_ref())?;
        Ok(opt_blob)
    }

    #[inline]
    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        let sample_res = self.handler.sample_put(ctx, &key, &value);
        self.inner.put(ctx, key, value).await?;
        sample_res
    }

    #[inline]
    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        let result = self.inner.is_present(ctx, key).await?;
        self.handler.sample_is_present(ctx, key, &result)?;

        Ok(result)
    }
}

/// Used when you need the BlobstoreId (where there is one) in the sample
pub trait ComponentSamplingHandler: std::fmt::Debug + Send + Sync {
    fn sample_get(
        &self,
        ctx: &CoreContext,
        key: &str,
        value: Option<&BlobstoreGetData>,
        inner_id: Option<BlobstoreId>,
    ) -> Result<()>;

    fn sample_put(
        &self,
        _ctx: &CoreContext,
        _key: &str,
        _value: &BlobstoreBytes,
        _inner_id: Option<BlobstoreId>,
    ) -> Result<()> {
        Ok(())
    }

    fn sample_is_present(
        &self,
        _ctx: &CoreContext,
        _key: &str,
        _value: &BlobstoreIsPresent,
        _inner_id: Option<BlobstoreId>,
    ) -> Result<()> {
        Ok(())
    }
}

/// A lower level sampler that can provide BlobstoreId
#[derive(Debug)]
pub struct SamplingBlobstorePutOps<T> {
    inner: T,
    inner_id: Option<BlobstoreId>,
    handler: Arc<dyn ComponentSamplingHandler>,
}

impl<T: std::fmt::Display> std::fmt::Display for SamplingBlobstorePutOps<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "SamplingBlobstorePutOps<{}, {:?}>",
            &self.inner, self.inner_id
        )
    }
}

impl<T> SamplingBlobstorePutOps<T> {
    pub fn new(
        inner: T,
        inner_id: Option<BlobstoreId>,
        handler: Arc<dyn ComponentSamplingHandler>,
    ) -> Self {
        Self {
            inner,
            inner_id,
            handler,
        }
    }
}

#[async_trait]
impl<T: Blobstore + BlobstorePutOps> Blobstore for SamplingBlobstorePutOps<T> {
    #[inline]
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let opt_blob = self.inner.get(ctx, key).await?;
        self.handler
            .sample_get(ctx, key, opt_blob.as_ref(), self.inner_id)?;
        Ok(opt_blob)
    }

    #[inline]
    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        let sample_res = self.handler.sample_put(ctx, &key, &value, self.inner_id);
        self.inner.put(ctx, key, value).await?;
        sample_res
    }

    #[inline]
    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        let result = self.inner.is_present(ctx, key).await?;
        self.handler
            .sample_is_present(ctx, key, &result, self.inner_id)?;

        Ok(result)
    }
}

#[async_trait]
impl<T: BlobstorePutOps> BlobstorePutOps for SamplingBlobstorePutOps<T> {
    async fn put_explicit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        self.handler.sample_put(ctx, &key, &value, self.inner_id)?;
        self.inner
            .put_explicit(ctx, key, value, put_behaviour)
            .await
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.handler.sample_put(ctx, &key, &value, self.inner_id)?;
        self.inner.put_with_status(ctx, key, value).await
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use borrowed::borrowed;
    use fbinit::FacebookInit;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    use context::SamplingKey;
    use memblob::Memblob;

    #[derive(Debug)]
    struct TestSamplingHandler {
        sampled: AtomicBool,
        looking_for: SamplingKey,
    }
    impl TestSamplingHandler {
        fn check_sample(&self, ctx: &CoreContext) -> Result<()> {
            if let Some(sampling_key) = ctx.sampling_key() {
                if sampling_key == &self.looking_for {
                    self.sampled.store(true, Ordering::Relaxed);
                }
            }
            Ok(())
        }
    }

    impl SamplingHandler for TestSamplingHandler {
        fn sample_get(
            &self,
            ctx: &CoreContext,
            _key: &str,
            _value: Option<&BlobstoreGetData>,
        ) -> Result<()> {
            self.check_sample(ctx)
        }
        fn sample_put(&self, ctx: &CoreContext, _key: &str, _value: &BlobstoreBytes) -> Result<()> {
            self.check_sample(ctx)
        }
        fn sample_is_present(
            &self,
            ctx: &CoreContext,
            _key: &str,
            _value: &BlobstoreIsPresent,
        ) -> Result<()> {
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
        let base_present = base
            .is_present(ctx, key)
            .await
            .unwrap()
            .assume_not_found_if_unsure();
        assert!(base_present);
        let was_sampled = handler.sampled.load(Ordering::Relaxed);
        assert!(!was_sampled);
        let wrapper_present = wrapper
            .is_present(ctx, key)
            .await
            .unwrap()
            .assume_not_found_if_unsure();
        assert!(wrapper_present);
        let was_sampled = handler.sampled.load(Ordering::Relaxed);
        assert!(was_sampled);
    }

    impl ComponentSamplingHandler for TestSamplingHandler {
        fn sample_get(
            &self,
            ctx: &CoreContext,
            _key: &str,
            _value: Option<&BlobstoreGetData>,
            _inner_id: Option<BlobstoreId>,
        ) -> Result<()> {
            self.check_sample(ctx)
        }
        fn sample_put(
            &self,
            ctx: &CoreContext,
            _key: &str,
            _value: &BlobstoreBytes,
            _inner_id: Option<BlobstoreId>,
        ) -> Result<()> {
            self.check_sample(ctx)
        }
        fn sample_is_present(
            &self,
            ctx: &CoreContext,
            _key: &str,
            _value: &BlobstoreIsPresent,
            _inner_id: Option<BlobstoreId>,
        ) -> Result<()> {
            self.check_sample(ctx)
        }
    }

    #[fbinit::test]
    async fn test_component_sample_called(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);
        let base = Memblob::default();
        let sample_this = SamplingKey::new();
        let handler = Arc::new(TestSamplingHandler {
            sampled: AtomicBool::new(false),
            looking_for: sample_this,
        });
        let wrapper = SamplingBlobstorePutOps::new(
            base.clone(),
            None,
            handler.clone() as Arc<dyn ComponentSamplingHandler>,
        );
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
        let base_present = base
            .is_present(ctx, key)
            .await
            .unwrap()
            .assume_not_found_if_unsure();
        assert!(base_present);
        let was_sampled = handler.sampled.load(Ordering::Relaxed);
        assert!(!was_sampled);
        let wrapper_present = wrapper
            .is_present(ctx, key)
            .await
            .unwrap()
            .assume_not_found_if_unsure();
        assert!(wrapper_present);
        let was_sampled = handler.sampled.load(Ordering::Relaxed);
        assert!(was_sampled);
    }
}

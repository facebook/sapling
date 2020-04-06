/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobstore::Blobstore;
use cloned::cloned;
use context::CoreContext;
use futures::future::Future;
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::BlobstoreBytes;
use std::sync::Arc;

pub trait SamplingHandler: std::fmt::Debug + Send + Sync {
    fn sample_get(&self, ctx: CoreContext, key: String, value: Option<&BlobstoreBytes>);
    fn sample_put(&self, _ctx: &CoreContext, _key: &str, _value: &BlobstoreBytes) {}
    fn sample_is_present(&self, _ctx: CoreContext, _key: String, _value: bool) {}
}

/// A layer over an existing blobstore that allows sampling of blobs, e.g. for
/// corpus generation.
#[derive(Clone, Debug)]
pub struct SamplingBlobstore<T: Blobstore + Clone> {
    inner: T,
    handler: Arc<dyn SamplingHandler>,
}

impl<T: Blobstore + Clone> SamplingBlobstore<T> {
    pub fn new(inner: T, handler: Arc<dyn SamplingHandler>) -> Self {
        Self { inner, handler }
    }
}

impl<T: Blobstore + Clone> Blobstore for SamplingBlobstore<T> {
    #[inline]
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.inner
            .get(ctx.clone(), key.clone())
            .map({
                cloned!(self.handler);
                move |opt_bytes| {
                    handler.sample_get(ctx, key, opt_bytes.as_ref());
                    opt_bytes
                }
            })
            .boxify()
    }

    #[inline]
    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        self.handler.sample_put(&ctx, &key, &value);
        self.inner.put(ctx, key, value)
    }

    #[inline]
    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        self.inner
            .is_present(ctx.clone(), key.clone())
            .map({
                cloned!(self.handler);
                move |is_present| {
                    handler.sample_is_present(ctx, key, is_present);
                    is_present
                }
            })
            .boxify()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use fbinit::FacebookInit;
    use futures::Future;
    use std::sync::atomic::{AtomicBool, Ordering};

    use context::SamplingKey;
    use memblob::EagerMemblob;

    #[derive(Debug)]
    struct TestSamplingHandler {
        sampled: AtomicBool,
        looking_for: SamplingKey,
    }
    impl TestSamplingHandler {
        fn check_sample(&self, ctx: &CoreContext) {
            ctx.sampling_key().map(|sampling_key| {
                if sampling_key == &self.looking_for {
                    self.sampled.store(true, Ordering::Relaxed);
                }
            });
        }
    }

    impl SamplingHandler for TestSamplingHandler {
        fn sample_get(&self, ctx: CoreContext, _key: String, _value: Option<&BlobstoreBytes>) {
            self.check_sample(&ctx);
        }
        fn sample_put(&self, ctx: &CoreContext, _key: &str, _value: &BlobstoreBytes) {
            self.check_sample(ctx);
        }
        fn sample_is_present(&self, ctx: CoreContext, _key: String, _value: bool) {
            self.check_sample(&ctx);
        }
    }

    #[fbinit::test]
    fn test_sample_called(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let base = EagerMemblob::new();
        let sample_this = SamplingKey::new();
        let handler = Arc::new(TestSamplingHandler {
            sampled: AtomicBool::new(false),
            looking_for: sample_this,
        });
        let wrapper =
            SamplingBlobstore::new(base.clone(), handler.clone() as Arc<dyn SamplingHandler>);
        let key = "foobar".to_string();

        // We're using EagerMemblob (immediate future completion) so calling wait() is fine.
        let r = wrapper
            .put(
                ctx.clone(),
                key.clone(),
                BlobstoreBytes::from_bytes("test foobar"),
            )
            .wait();
        assert!(r.is_ok());
        let was_sampled = handler.sampled.load(Ordering::Relaxed);
        assert!(!was_sampled);
        let ctx = ctx.clone_and_sample(sample_this);
        let base_present = base.is_present(ctx.clone(), key.clone()).wait().unwrap();
        assert!(base_present);
        let was_sampled = handler.sampled.load(Ordering::Relaxed);
        assert!(!was_sampled);
        let wrapper_present = wrapper.is_present(ctx, key.clone()).wait().unwrap();
        assert!(wrapper_present);
        let was_sampled = handler.sampled.load(Ordering::Relaxed);
        assert!(was_sampled);
    }
}

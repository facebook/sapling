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
use mononoke_types::BlobstoreBytes;
use rand::thread_rng;
use rand::Rng;
use std::num::NonZeroU32;

mod errors;
pub use crate::errors::ErrorKind;

const NEVER_CHAOS_THRESHOLD: f32 = 1.0;
const ALWAYS_CHAOS_THRESHOLD: f32 = -1.0;

#[derive(Clone, Copy, Debug)]
pub struct ChaosOptions {
    error_sample_read: Option<NonZeroU32>,
    error_sample_write: Option<NonZeroU32>,
}

impl ChaosOptions {
    /// Pass `error_sample_read` or `error_sample_write` value
    /// from Some(1) for always chaos to Some(N) to get 1/N chance of failure.
    pub fn new(
        error_sample_read: Option<NonZeroU32>,
        error_sample_write: Option<NonZeroU32>,
    ) -> Self {
        Self {
            error_sample_read,
            error_sample_write,
        }
    }

    pub fn has_chaos(&self) -> bool {
        self.error_sample_read.is_some() || self.error_sample_write.is_some()
    }
}

/// A layer over an existing blobstore that errors randomly
#[derive(Clone, Debug)]
pub struct ChaosBlobstore<T> {
    blobstore: T,
    sample_threshold_read: f32,
    sample_threshold_write: f32,
    #[allow(dead_code)]
    options: ChaosOptions,
}

impl<T: std::fmt::Display> std::fmt::Display for ChaosBlobstore<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ChaosBlobstore<{}>", &self.blobstore)
    }
}

fn derive_threshold(sample_rate: Option<NonZeroU32>) -> f32 {
    sample_rate.map_or(NEVER_CHAOS_THRESHOLD, |rate| {
        match rate.get() {
            // Avoid chance of rng returning 0.0 and threshold being 0.0
            1 => ALWAYS_CHAOS_THRESHOLD,
            // If rate 100, then rng must generate over 0.99 to trigger error
            n => 1.0 - (1.0 / (n as f32)),
        }
    })
}

impl<T> ChaosBlobstore<T> {
    pub fn new(blobstore: T, options: ChaosOptions) -> Self {
        let sample_threshold_read = derive_threshold(options.error_sample_read);
        let sample_threshold_write = derive_threshold(options.error_sample_write);
        Self {
            blobstore,
            sample_threshold_read,
            sample_threshold_write,
            options,
        }
    }
}

#[async_trait]
impl<T: Blobstore + BlobstorePutOps> Blobstore for ChaosBlobstore<T> {
    #[inline]
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let should_error = thread_rng().gen::<f32>() > self.sample_threshold_read;
        let get = self.blobstore.get(ctx, key);
        if should_error {
            Err(ErrorKind::InjectedChaosGet(key.to_owned()).into())
        } else {
            get.await
        }
    }

    #[inline]
    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        self.put_impl(ctx, key, value, None).await?;
        Ok(())
    }

    #[inline]
    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        let should_error = thread_rng().gen::<f32>() > self.sample_threshold_read;
        let is_present = self.blobstore.is_present(ctx, key);
        if should_error {
            Err(ErrorKind::InjectedChaosIsPresent(key.to_owned()).into())
        } else {
            is_present.await
        }
    }
}

impl<T: BlobstorePutOps> ChaosBlobstore<T> {
    async fn put_impl<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: Option<PutBehaviour>,
    ) -> Result<OverwriteStatus> {
        let should_error = thread_rng().gen::<f32>() > self.sample_threshold_write;
        let put = if should_error {
            None
        } else {
            let put = if let Some(put_behaviour) = put_behaviour {
                self.blobstore
                    .put_explicit(ctx, key.clone(), value, put_behaviour)
            } else {
                self.blobstore.put_with_status(ctx, key.clone(), value)
            };
            Some(put)
        };
        match put {
            None => Err(ErrorKind::InjectedChaosPut(key).into()),
            Some(put) => put.await,
        }
    }
}

#[async_trait]
impl<T: BlobstorePutOps> BlobstorePutOps for ChaosBlobstore<T> {
    async fn put_explicit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        self.put_impl(ctx, key, value, Some(put_behaviour)).await
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.put_impl(ctx, key, value, None).await
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use borrowed::borrowed;
    use fbinit::FacebookInit;

    use memblob::Memblob;

    #[fbinit::test]
    async fn test_error_on_write(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);
        let base = Memblob::default();
        let wrapper =
            ChaosBlobstore::new(base.clone(), ChaosOptions::new(None, NonZeroU32::new(1)));
        let key = "foobar";

        let r = wrapper
            .put(
                ctx,
                key.to_owned(),
                BlobstoreBytes::from_bytes("test foobar"),
            )
            .await;
        assert!(r.is_err());
        let base_present = base
            .is_present(ctx, key)
            .await
            .unwrap()
            .assume_not_found_if_unsure();
        assert!(!base_present);
    }

    #[fbinit::test]
    async fn test_error_on_write_with_status(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);
        let base = Memblob::default();
        let wrapper =
            ChaosBlobstore::new(base.clone(), ChaosOptions::new(None, NonZeroU32::new(1)));
        let key = "foobar";

        let r = wrapper
            .put_with_status(
                ctx,
                key.to_owned(),
                BlobstoreBytes::from_bytes("test foobar"),
            )
            .await;
        assert!(r.is_err());
        let base_present = base
            .is_present(ctx, key)
            .await
            .unwrap()
            .assume_not_found_if_unsure();
        assert!(!base_present);
    }

    #[fbinit::test]
    async fn test_error_on_read(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);
        let base = Memblob::default();
        let wrapper =
            ChaosBlobstore::new(base.clone(), ChaosOptions::new(NonZeroU32::new(1), None));
        let key = "foobar";

        let r = wrapper
            .put(
                ctx,
                key.to_owned(),
                BlobstoreBytes::from_bytes("test foobar"),
            )
            .await;
        assert!(r.is_ok());
        let base_present = base
            .is_present(ctx, key)
            .await
            .unwrap()
            .assume_not_found_if_unsure();
        assert!(base_present);
        let r = wrapper.get(ctx, key).await;
        assert!(r.is_err());
    }
}

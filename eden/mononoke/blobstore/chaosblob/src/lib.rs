/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use blobstore::Blobstore;
use context::CoreContext;
use futures::future;
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::BlobstoreBytes;
use rand::{thread_rng, Rng};
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
pub struct ChaosBlobstore<T: Blobstore + Clone> {
    blobstore: T,
    sample_threshold_read: f32,
    sample_threshold_write: f32,
    options: ChaosOptions,
}

fn derive_threshold(sample_rate: Option<NonZeroU32>) -> f32 {
    sample_rate
        .map(|rate| {
            match rate.get() {
                // Avoid chance of rng returning 0.0 and threshold being 0.0
                1 => ALWAYS_CHAOS_THRESHOLD,
                // If rate 100, then rng must generate over 0.99 to trigger error
                n => 1.0 - (1.0 / (n as f32)),
            }
        })
        .unwrap_or(NEVER_CHAOS_THRESHOLD)
}

impl<T: Blobstore + Clone> ChaosBlobstore<T> {
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

impl<T: Blobstore + Clone> Blobstore for ChaosBlobstore<T> {
    #[inline]
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        let should_error = thread_rng().gen::<f32>() > self.sample_threshold_read;
        if should_error {
            future::err(ErrorKind::InjectedChaosGet(key).into()).boxify()
        } else {
            self.blobstore.get(ctx, key)
        }
    }

    #[inline]
    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        let should_error = thread_rng().gen::<f32>() > self.sample_threshold_write;
        if should_error {
            future::err(ErrorKind::InjectedChaosPut(key).into()).boxify()
        } else {
            self.blobstore.put(ctx, key, value)
        }
    }

    #[inline]
    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        let should_error = thread_rng().gen::<f32>() > self.sample_threshold_read;
        if should_error {
            future::err(ErrorKind::InjectedChaosIsPresent(key).into()).boxify()
        } else {
            self.blobstore.is_present(ctx, key)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use fbinit::FacebookInit;
    use futures::Future;

    use memblob::EagerMemblob;

    #[fbinit::test]
    fn test_error_on_write(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let base = EagerMemblob::new();
        let wrapper =
            ChaosBlobstore::new(base.clone(), ChaosOptions::new(None, NonZeroU32::new(1)));
        let key = "foobar".to_string();

        // We're using EagerMemblob (immediate future completion) so calling wait() is fine.
        let r = wrapper
            .put(
                ctx.clone(),
                key.clone(),
                BlobstoreBytes::from_bytes("test foobar"),
            )
            .wait();
        assert!(!r.is_ok());
        let base_present = base.is_present(ctx, key.clone()).wait().unwrap();
        assert!(!base_present);
    }

    #[fbinit::test]
    fn test_error_on_read(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let base = EagerMemblob::new();
        let wrapper =
            ChaosBlobstore::new(base.clone(), ChaosOptions::new(NonZeroU32::new(1), None));
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
        let base_present = base.is_present(ctx.clone(), key.clone()).wait().unwrap();
        assert!(base_present);
        let r = wrapper.get(ctx.clone(), key.clone()).wait();
        assert!(!r.is_ok());
    }
}

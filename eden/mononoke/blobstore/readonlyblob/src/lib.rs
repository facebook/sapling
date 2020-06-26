/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobstore::{Blobstore, BlobstoreGetData};
use context::CoreContext;
use futures::future::{self, BoxFuture, FutureExt};
use mononoke_types::BlobstoreBytes;
mod errors;
pub use crate::errors::ErrorKind;

/// A layer over an existing blobstore that prevents writes.
#[derive(Clone, Debug)]
pub struct ReadOnlyBlobstore<T: Blobstore + Clone> {
    blobstore: T,
}

impl<T: Blobstore + Clone> ReadOnlyBlobstore<T> {
    pub fn new(blobstore: T) -> Self {
        Self { blobstore }
    }
}

impl<T: Blobstore + Clone> Blobstore for ReadOnlyBlobstore<T> {
    #[inline]
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        self.blobstore.get(ctx, key)
    }

    #[inline]
    fn put(
        &self,
        _ctx: CoreContext,
        key: String,
        _value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        future::err(ErrorKind::ReadOnlyPut(key).into()).boxed()
    }

    #[inline]
    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'static, Result<bool, Error>> {
        self.blobstore.is_present(ctx, key)
    }

    #[inline]
    fn assert_present(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<(), Error>> {
        self.blobstore.assert_present(ctx, key)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use fbinit::FacebookInit;

    use memblob::EagerMemblob;

    #[fbinit::compat_test]
    async fn test_error_on_write(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let base = EagerMemblob::new();
        let wrapper = ReadOnlyBlobstore::new(base.clone());
        let key = "foobar".to_string();

        // We're using EagerMemblob (immediate future completion) so calling wait() is fine.
        let r = wrapper
            .put(
                ctx.clone(),
                key.clone(),
                BlobstoreBytes::from_bytes("test foobar"),
            )
            .await;
        assert!(!r.is_ok());
        let base_present = base.is_present(ctx, key.clone()).await.unwrap();
        assert!(!base_present);
    }
}

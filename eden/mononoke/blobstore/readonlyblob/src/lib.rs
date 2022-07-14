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
mod errors;
pub use crate::errors::ErrorKind;

/// A layer over an existing blobstore that prevents writes.
#[derive(Debug)]
pub struct ReadOnlyBlobstore<T> {
    blobstore: T,
}

impl<T: std::fmt::Display> std::fmt::Display for ReadOnlyBlobstore<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ReadOnlyBlobstore<{}>", &self.blobstore)
    }
}

impl<T> ReadOnlyBlobstore<T> {
    pub fn new(blobstore: T) -> Self {
        Self { blobstore }
    }
}

#[async_trait]
impl<T: Blobstore> Blobstore for ReadOnlyBlobstore<T> {
    #[inline]
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.blobstore.get(ctx, key).await
    }

    #[inline]
    async fn put<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: String,
        _value: BlobstoreBytes,
    ) -> Result<()> {
        Err(ErrorKind::ReadOnlyPut(key).into())
    }

    #[inline]
    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        self.blobstore.is_present(ctx, key).await
    }
}

#[async_trait]
impl<T: BlobstorePutOps> BlobstorePutOps for ReadOnlyBlobstore<T> {
    async fn put_explicit<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: String,
        _value: BlobstoreBytes,
        _put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        Err(ErrorKind::ReadOnlyPut(key).into())
    }

    async fn put_with_status<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: String,
        _value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        Err(ErrorKind::ReadOnlyPut(key).into())
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
        let wrapper = ReadOnlyBlobstore::new(base.clone());
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
    async fn test_error_on_put_with_status(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);
        let base = Memblob::default();
        let wrapper = ReadOnlyBlobstore::new(base.clone());
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
}

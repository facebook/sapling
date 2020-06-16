/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::envelope::PackEnvelope;

use anyhow::{format_err, Error};
use blobstore::{Blobstore, BlobstoreGetData};
use context::CoreContext;
use futures::{compat::Future01CompatExt, FutureExt, TryFutureExt};
use futures_ext::{BoxFuture as BoxFuture01, FutureExt as Future01Ext};
use mononoke_types::BlobstoreBytes;
use packblob_thrift::{SingleValue, StorageEnvelope, StorageFormat};
use std::convert::TryInto;

/// A layer over an existing blobstore that uses thrift blob wrappers to allow packing and compression
#[derive(Clone, Debug)]
pub struct PackBlob<T: Blobstore + Clone> {
    inner: T,
}

impl<T: Blobstore + Clone> PackBlob<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
}

// differentiate keys just in case packblob is run in an existing unpacked store
const ENVELOPE_SUFFIX: &str = ".pack";

impl<T: Blobstore + Clone> Blobstore for PackBlob<T> {
    fn get(
        &self,
        ctx: CoreContext,
        mut key: String,
    ) -> BoxFuture01<Option<BlobstoreGetData>, Error> {
        key.push_str(ENVELOPE_SUFFIX);
        let inner_get_data = self.inner.get(ctx, key);
        async move {
            let inner_get_data = match inner_get_data.compat().await? {
                Some(inner_get_data) => inner_get_data,
                None => return Ok(None),
            };

            let meta = inner_get_data.as_meta().clone();
            let envelope: PackEnvelope = inner_get_data.into_bytes().try_into()?;

            let get_data = match envelope.0.storage {
                StorageFormat::Single(SingleValue::Raw(v)) => {
                    Some(BlobstoreGetData::new(meta, BlobstoreBytes::from_bytes(v)))
                }
                e => return Err(format_err!("Unexpected StorageFormat {:?}", e)),
            };

            Ok(get_data)
        }
        .boxed()
        .compat()
        .boxify()
    }

    fn put(
        &self,
        ctx: CoreContext,
        mut key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture01<(), Error> {
        key.push_str(ENVELOPE_SUFFIX);
        // Wrap in thrift encoding
        let envelope: PackEnvelope = PackEnvelope(StorageEnvelope {
            storage: StorageFormat::Single(SingleValue::Raw(value.into_bytes().to_vec())),
        });
        // pass through the put after wrapping
        self.inner.put(ctx, key, envelope.into())
    }

    fn is_present(&self, ctx: CoreContext, mut key: String) -> BoxFuture01<bool, Error> {
        key.push_str(ENVELOPE_SUFFIX);
        self.inner.is_present(ctx, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use fbinit::FacebookInit;
    use memblob::EagerMemblob;
    use std::sync::Arc;

    #[fbinit::compat_test]
    async fn simple_roundtrip_test(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let inner_blobstore = Arc::new(EagerMemblob::new());

        let outer_key = "repofoo.randomkey".to_string();

        let packblob = PackBlob::new(inner_blobstore.clone());

        let value = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(b"appleveldata"));

        // Put, this will apply the thrift envelope and save to the inner store
        packblob
            .put(ctx.clone(), outer_key.clone(), value.clone())
            .compat()
            .await?;

        // Get, should remove the thrift envelope as it is loaded
        let fetched_value = packblob
            .get(ctx.clone(), outer_key.clone())
            .compat()
            .await?
            .unwrap();

        // Make sure the thrift wrapper is not still there!
        assert_eq!(value, fetched_value.into_bytes());

        // Make sure that inner blobstore stores has packed value (i.e. not equal to what was written)
        let mut inner_key = outer_key.clone();
        inner_key.push_str(ENVELOPE_SUFFIX);
        let fetched_value = inner_blobstore
            .get(ctx.clone(), inner_key.clone())
            .compat()
            .await?
            .unwrap();

        assert_ne!(value, fetched_value.into_bytes());

        // Check is_present matches
        let is_present = inner_blobstore
            .is_present(ctx.clone(), inner_key)
            .compat()
            .await?;
        assert!(is_present);

        // Check the key without suffix is not there
        let is_not_present = !inner_blobstore
            .is_present(ctx.clone(), outer_key)
            .compat()
            .await?;
        assert!(is_not_present);

        Ok(())
    }
}

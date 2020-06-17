/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::envelope::PackEnvelope;
use crate::pack;

use anyhow::{format_err, Context, Error};
use blobstore::{Blobstore, BlobstoreGetData, BlobstoreWithLink};
use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    stream::{FuturesUnordered, TryStreamExt},
    FutureExt, TryFutureExt,
};
use futures_ext::{BoxFuture as BoxFuture01, FutureExt as Future01Ext};
use mononoke_types::BlobstoreBytes;
use packblob_thrift::{PackedEntry, SingleValue, StorageEnvelope, StorageFormat};
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
pub const ENVELOPE_SUFFIX: &str = ".pack";

impl<T: Blobstore + Clone> Blobstore for PackBlob<T> {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture01<Option<BlobstoreGetData>, Error> {
        let inner_get_data = {
            let mut inner_key = key.clone();
            inner_key.push_str(ENVELOPE_SUFFIX);
            self.inner.get(ctx, inner_key)
        };
        async move {
            let inner_get_data = match inner_get_data
                .compat()
                .await
                .with_context(|| format!("While getting inner data for {:?}", key))?
            {
                Some(inner_get_data) => inner_get_data,
                None => return Ok(None),
            };

            let meta = inner_get_data.as_meta().clone();
            let envelope: PackEnvelope = inner_get_data.into_bytes().try_into()?;

            let get_data = match envelope.0.storage {
                StorageFormat::Single(single) => pack::decode_independent(meta, single)
                    .with_context(|| format!("While decoding independent {:?}", key))?,
                StorageFormat::Packed(packed) => pack::decode_pack(meta, packed, key.clone())
                    .with_context(|| format!("While decoding pack for {:?}", key))?,
                StorageFormat::UnknownField(e) => {
                    return Err(format_err!("StorageFormat::UnknownField {:?}", e))
                }
            };

            Ok(Some(get_data))
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

impl<T: Blobstore + BlobstoreWithLink + Clone> PackBlob<T> {
    // Put packed content, returning the pack's key if successful.
    // `prefix` is in the control of the packer, e.g. if packing only
    // filecontent together packer can chose "repoXXXX.packed.file_content."
    //
    // On ref counted stores the packer will need to call unlink on the returned key
    // if its desirable for old packs to be removed.
    pub async fn put_packed(
        &self,
        ctx: CoreContext,
        entries: Vec<PackedEntry>,
        prefix: String,
    ) -> Result<String, Error> {
        let link_keys: Vec<String> = entries.iter().map(|entry| entry.key.clone()).collect();

        let pack = pack::create_packed(entries)
            .with_context(|| format!("While packing entries for {:?}", link_keys.clone()))?;

        let mut pack_key = prefix;
        pack_key.push_str(&pack.key);

        // Wrap in thrift encoding
        let pack = PackEnvelope(StorageEnvelope {
            storage: StorageFormat::Packed(pack),
        });

        // pass through the put after wrapping
        self.inner
            .put(ctx.clone(), pack_key.clone(), pack.into())
            .compat()
            .await?;

        // add the links
        let links = FuturesUnordered::new();
        for mut key in link_keys {
            key.push_str(ENVELOPE_SUFFIX);
            links.push(self.inner.link(ctx.clone(), pack_key.clone(), key));
        }
        links.try_collect().await?;

        Ok(pack_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use fbinit::FacebookInit;
    use memblob::EagerMemblob;
    use packblob_thrift::{PackedEntry, PackedValue, SingleValue};
    use std::sync::Arc;

    #[fbinit::compat_test]
    async fn simple_roundtrip_test(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let inner_blobstore = Arc::new(EagerMemblob::new());

        let outer_key = "repo0000.randomkey".to_string();

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

    #[fbinit::compat_test]
    async fn simple_pack_test(fb: FacebookInit) -> Result<(), Error> {
        let mut input_entries = vec![];
        let mut input_values = vec![];
        for i in 0..3 {
            let mut app_key = "repo0000.app_key".to_string();
            app_key.push_str(&i.to_string());

            let app_data = format!("app_data{}", i);
            let app_data = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(app_data.as_bytes()));
            input_values.push(app_data.clone());

            input_entries.push(PackedEntry {
                key: app_key,
                data: PackedValue::Single(SingleValue::Raw(app_data.into_bytes().to_vec())),
            })
        }

        let ctx = CoreContext::test_mock(fb);
        let inner_blobstore = EagerMemblob::new();
        let packblob = PackBlob::new(inner_blobstore.clone());

        // put_packed, this will apply the thrift envelope and save to the inner store
        let inner_key = packblob
            .put_packed(
                ctx.clone(),
                input_entries.clone(),
                "repo0000.packed_app_data.".to_string(),
            )
            .await?;

        // Check the inner key is present (as we haven't unlinked it yet)
        let is_present = inner_blobstore
            .is_present(ctx.clone(), inner_key)
            .compat()
            .await?;
        assert!(is_present);

        // Get, should remove the thrift envelope as it is loaded
        let fetched_value = packblob
            .get(ctx.clone(), input_entries[1].key.clone())
            .compat()
            .await?;

        assert!(fetched_value.is_some());

        // Make sure the thrift wrapper is not still there
        assert_eq!(input_values[1], fetched_value.unwrap().into_bytes());

        Ok(())
    }
}

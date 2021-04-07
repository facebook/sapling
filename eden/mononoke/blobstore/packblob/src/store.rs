/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::envelope::PackEnvelope;
use crate::pack;

use anyhow::{Context, Result};
use async_trait::async_trait;
use blobstore::{
    Blobstore, BlobstoreGetData, BlobstorePutOps, BlobstoreWithLink, OverwriteStatus, PutBehaviour,
};
use bytes::Bytes;
use context::CoreContext;
use futures::stream::{FuturesUnordered, TryStreamExt};
use metaconfig_types::PackFormat;
use mononoke_types::BlobstoreBytes;
use packblob_thrift::{SingleValue, StorageEnvelope, StorageFormat};
use std::{convert::TryInto, io::Cursor};

#[derive(Clone, Debug, Default)]
pub struct PackOptions {
    // None - user didn't specify
    // Some(xxx) - user wants to override config
    pub override_put_format: Option<PackFormat>,
}

impl PackOptions {
    pub fn new(override_put_format: Option<PackFormat>) -> Self {
        Self {
            override_put_format,
        }
    }
}

/// A layer over an existing blobstore that uses thrift blob wrappers to allow packing and compression
#[derive(Debug)]
pub struct PackBlob<T> {
    inner: T,
    put_format: PackFormat,
}

impl<T: std::fmt::Display> std::fmt::Display for PackBlob<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PackBlob<{}>", &self.inner)
    }
}

impl<T> PackBlob<T> {
    pub fn new(inner: T, put_format: PackFormat) -> Self {
        Self { inner, put_format }
    }
}

// If compressed version is smaller, use it, otherwise return raw
fn compress_if_worthwhile(value: Bytes, zstd_level: i32) -> Result<SingleValue> {
    let cursor = Cursor::new(value.clone());
    let compressed = zstd::encode_all(cursor, zstd_level)?;
    if compressed.len() < value.len() {
        Ok(SingleValue::Zstd(Bytes::from(compressed)))
    } else {
        Ok(SingleValue::Raw(value))
    }
}

// differentiate keys just in case packblob is run in an existing unpacked store
pub const ENVELOPE_SUFFIX: &str = ".pack";

#[async_trait]
impl<T: Blobstore + BlobstorePutOps> Blobstore for PackBlob<T> {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let inner_get_data = {
            let inner_key = &[key, ENVELOPE_SUFFIX].concat();
            self.inner
                .get(ctx, &inner_key)
                .await
                .with_context(|| format!("While getting inner data for {:?}", key))?
        };
        let inner_get_data = match inner_get_data {
            Some(inner_get_data) => inner_get_data,
            None => return Ok(None),
        };

        let meta = inner_get_data.as_meta().clone();
        let envelope: PackEnvelope = inner_get_data.into_bytes().try_into()?;
        Ok(Some(BlobstoreGetData::new(meta, envelope.decode(key)?)))
    }

    async fn is_present<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<bool> {
        self.inner
            .is_present(ctx, &[key, ENVELOPE_SUFFIX].concat())
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

impl<T: BlobstorePutOps> PackBlob<T> {
    async fn put_impl<'a>(
        &'a self,
        ctx: &'a CoreContext,
        mut key: String,
        value: BlobstoreBytes,
        put_behaviour: Option<PutBehaviour>,
    ) -> Result<OverwriteStatus> {
        key.push_str(ENVELOPE_SUFFIX);

        let value = value.into_bytes();

        let single = match self.put_format {
            PackFormat::ZstdIndividual(zstd_level) => compress_if_worthwhile(value, zstd_level)?,
            PackFormat::Raw => SingleValue::Raw(value),
        };

        // Wrap in thrift encoding
        let envelope: PackEnvelope = PackEnvelope(StorageEnvelope {
            storage: StorageFormat::Single(single),
        });
        // pass through the put after wrapping
        if let Some(put_behaviour) = put_behaviour {
            self.inner
                .put_explicit(ctx, key, envelope.into(), put_behaviour)
                .await
        } else {
            self.inner.put_with_status(ctx, key, envelope.into()).await
        }
    }
}

#[async_trait]
impl<B: BlobstorePutOps> BlobstorePutOps for PackBlob<B> {
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

impl<T: Blobstore + BlobstoreWithLink> PackBlob<T> {
    // Put packed content, returning the pack's key if successful.
    // `prefix` is in the control of the packer, e.g. if packing only
    // filecontent together packer can chose "repoXXXX.packed.file_content."
    pub async fn put_packed<'a>(
        &'a self,
        ctx: &'a CoreContext,
        pack: pack::Pack,
        repo_prefix: String,
        prefix: String,
    ) -> Result<String> {
        let (pack_key, link_keys, blob) = pack.into_blobstore_bytes(prefix)?;

        // pass through the put after wrapping
        self.inner.put(ctx, pack_key.clone(), blob).await?;

        // add the links
        let links = FuturesUnordered::new();
        for key in link_keys {
            let key = format!("{}{}{}", repo_prefix, key, ENVELOPE_SUFFIX);
            links.push(self.inner.link(ctx, &pack_key, key));
        }
        links.try_collect().await?;

        // remove the pack key, so that only the entries links are keeping it live
        self.inner.unlink(ctx, &pack_key).await?;

        Ok(pack_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use borrowed::borrowed;
    use bytes::Bytes;
    use fbinit::FacebookInit;
    use memblob::Memblob;
    use rand::{RngCore, SeedableRng};
    use rand_xorshift::XorShiftRng;
    use std::sync::Arc;

    #[fbinit::test]
    async fn simple_roundtrip_test(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);
        let inner_blobstore = Arc::new(Memblob::default());
        let packblob = PackBlob::new(inner_blobstore.clone(), PackFormat::Raw);

        let outer_key = "repo0000.randomkey";
        let value = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(b"appleveldata"));
        let _ = roundtrip(ctx, inner_blobstore.clone(), &packblob, outer_key, value).await?;
        Ok(())
    }

    #[fbinit::test]
    async fn compressible_roundtrip_test(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);
        let innerblob = Arc::new(Memblob::default());
        let packblob = PackBlob::new(innerblob.clone(), PackFormat::ZstdIndividual(0));

        let bytes_in = Bytes::from(vec![7u8; 65535]);
        let value = BlobstoreBytes::from_bytes(bytes_in.clone());

        let outer_key = "repo0000.compressible";
        let inner_key = roundtrip(ctx, innerblob.clone(), &packblob, outer_key, value).await?;

        // check inner value is smaller
        let inner_value = innerblob.get(ctx, &inner_key).await?;
        assert!(inner_value.unwrap().into_bytes().len() < bytes_in.len());
        Ok(())
    }

    #[fbinit::test]
    async fn incompressible_roundtrip_test(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);
        let innerblob = Arc::new(Memblob::default());
        let packblob = PackBlob::new(innerblob.clone(), PackFormat::ZstdIndividual(0));

        let mut rng = XorShiftRng::seed_from_u64(0); // reproducable Rng
        let mut bytes_in = vec![7u8; 65535];
        rng.fill_bytes(&mut bytes_in);
        let bytes_in = Bytes::from(bytes_in);

        let outer_key = "repo0000.incompressible";
        let value = BlobstoreBytes::from_bytes(bytes_in.clone());
        let inner_key = roundtrip(ctx, innerblob.clone(), &packblob, outer_key, value).await?;

        // check inner value is larger (due to being raw plus thrift encoding)
        let inner_value = innerblob.get(ctx, &inner_key).await?;
        assert!(inner_value.unwrap().into_bytes().len() > bytes_in.len());
        Ok(())
    }

    async fn roundtrip(
        ctx: &CoreContext,
        inner_blobstore: Arc<Memblob>,
        packblob: &PackBlob<Arc<Memblob>>,
        outer_key: &str,
        value: BlobstoreBytes,
    ) -> Result<String> {
        // Put, this will apply the thrift envelope and save to the inner store
        packblob
            .put(ctx, outer_key.to_owned(), value.clone())
            .await?;

        // Get, should remove the thrift envelope as it is loaded
        let fetched_value = packblob.get(ctx, outer_key).await?.unwrap();

        // Make sure the thrift wrapper is not still there!
        assert_eq!(value, fetched_value.into_bytes());

        // Make sure that inner blobstore stores has packed value (i.e. not equal to what was written)
        let inner_key = &[outer_key, ENVELOPE_SUFFIX].concat();
        let fetched_value = inner_blobstore.get(ctx, inner_key).await?.unwrap();

        assert_ne!(value, fetched_value.into_bytes());

        // Check is_present matches
        let is_present = inner_blobstore.is_present(ctx, inner_key).await?;
        assert!(is_present);

        // Check the key without suffix is not there
        let is_not_present = !inner_blobstore.is_present(ctx, outer_key).await?;
        assert!(is_not_present);

        Ok(inner_key.to_owned())
    }

    #[fbinit::test]
    async fn simple_pack_test(fb: FacebookInit) -> Result<()> {
        let mut input_values = vec![];
        let pack = pack::EmptyPack::new(0);

        let base_key = "app_key0".to_string();
        let base_data = BlobstoreBytes::from_bytes(b"app_data0" as &[u8]);
        input_values.push(base_data.clone());

        let mut pack = pack.add_base_blob(base_key.clone(), base_data)?;
        for i in 1..3 {
            let mut app_key = "app_key".to_string();
            app_key.push_str(&i.to_string());

            let app_data = format!("app_data{}", i);
            let app_data = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(app_data.as_bytes()));
            input_values.push(app_data.clone());

            pack.add_delta_blob(base_key.clone(), app_key, app_data)?;
        }

        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);
        let inner_blobstore = Memblob::default();
        let packblob = PackBlob::new(inner_blobstore.clone(), PackFormat::Raw);

        // put_packed, this will apply the thrift envelope and save to the inner store
        let inner_key = packblob
            .put_packed(
                ctx,
                pack,
                "repo0000.".to_string(),
                "repo0000.packed_app_data.".to_string(),
            )
            .await?;

        // Check the inner key is not visible, the pack operation unlinks it
        let is_present = inner_blobstore.is_present(ctx, &inner_key).await?;
        assert!(!is_present);

        for (expected, i) in input_values.into_iter().zip(0..3usize) {
            // Get, should remove the thrift envelope as it is loaded
            let fetched_value = packblob.get(ctx, &format!("repo0000.app_key{}", i)).await?;

            assert!(
                fetched_value.is_some(),
                "Failed to fetch repo0000.app_key{}",
                i
            );

            // Make sure the thrift wrapper is not still there
            assert_eq!(expected, fetched_value.unwrap().into_bytes());
        }
        Ok(())
    }
}

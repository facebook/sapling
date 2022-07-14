/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::envelope::PackEnvelope;
use crate::pack;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreEnumerationData;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstoreKeyParam;
use blobstore::BlobstoreKeySource;
use blobstore::BlobstoreMetadata;
use blobstore::BlobstorePutOps;
use blobstore::BlobstoreUnlinkOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use context::CoreContext;
use futures::stream::FuturesUnordered;
use futures::stream::TryStreamExt;
use metaconfig_types::PackFormat;
use mononoke_types::BlobstoreBytes;

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
                .get(ctx, inner_key)
                .await
                .with_context(|| format!("While getting inner data for {:?}", key))?
        };
        let inner_get_data = match inner_get_data {
            Some(inner_get_data) => inner_get_data,
            None => return Ok(None),
        };

        let ctime = inner_get_data.as_meta().ctime();
        let envelope: PackEnvelope = inner_get_data.into_bytes().try_into()?;
        let (decoded, sizing) = envelope.decode(key)?;
        let meta = BlobstoreMetadata::new(ctime, Some(sizing));
        Ok(Some(BlobstoreGetData::new(meta, decoded)))
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
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

        let bytes = match self.put_format {
            PackFormat::ZstdIndividual(zstd_level) => {
                pack::SingleCompressed::new(zstd_level, value)?
            }
            PackFormat::Raw => pack::SingleCompressed::new_uncompressed(value),
        }
        .into_blobstore_bytes();

        // pass through the put after wrapping
        if let Some(put_behaviour) = put_behaviour {
            self.inner
                .put_explicit(ctx, key, bytes, put_behaviour)
                .await
        } else {
            self.inner.put_with_status(ctx, key, bytes).await
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

#[async_trait]
impl<B: BlobstoreKeySource + BlobstorePutOps> BlobstoreKeySource for PackBlob<B> {
    async fn enumerate<'a>(
        &'a self,
        ctx: &'a CoreContext,
        range: &'a BlobstoreKeyParam,
    ) -> Result<BlobstoreEnumerationData> {
        let mut enumeration = self.inner.enumerate(ctx, range).await?;
        // Include only keys with the envelope suffix, and remove the suffix
        // from those keys.
        enumeration.keys = enumeration
            .keys
            .into_iter()
            .filter_map(|mut key| {
                if key.ends_with(ENVELOPE_SUFFIX) {
                    let new_len = key.len() - ENVELOPE_SUFFIX.len();
                    key.truncate(new_len);
                    Some(key)
                } else {
                    None
                }
            })
            .collect();
        Ok(enumeration)
    }
}

#[async_trait]
impl<T: BlobstoreUnlinkOps> BlobstoreUnlinkOps for PackBlob<T> {
    async fn unlink<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        let inner_key = &[key, ENVELOPE_SUFFIX].concat();
        self.inner.unlink(ctx, inner_key).await
    }
}

impl<T: Blobstore + BlobstoreUnlinkOps> PackBlob<T> {
    /// Put packed content, returning the pack's key if successful.
    ///
    /// `key_prefix` is prefixed to all keys within the pack and used to
    /// create links to the pack for each packed key.
    ///
    /// `pack_prefix` is prefixed to the pack key, and is under the control of
    /// the packer, e.g. if packing only filecontent together packer can chose
    /// "repoXXXX.packed.file_content.".  It is used for the temporary pack
    /// file name and stored within the pack itself.
    pub async fn put_packed<'a>(
        &'a self,
        ctx: &'a CoreContext,
        pack: pack::Pack,
        key_prefix: String,
        pack_prefix: String,
    ) -> Result<String> {
        let (pack_key, link_keys, blob) = pack.into_blobstore_bytes(pack_prefix)?;

        // pass through the put after wrapping
        self.inner.put(ctx, pack_key.clone(), blob).await?;

        // add the links
        let links = FuturesUnordered::new();
        for key in link_keys {
            let key = format!("{}{}{}", key_prefix, key, ENVELOPE_SUFFIX);
            links.push(self.inner.copy(ctx, &pack_key, key));
        }
        links.try_collect().await?;

        // remove the pack key, so that only the entries links are keeping it live
        self.inner.unlink(ctx, &pack_key).await?;

        Ok(pack_key)
    }

    pub async fn put_single<'a>(
        &'a self,
        ctx: &'a CoreContext,
        mut key: String,
        value: pack::SingleCompressed,
    ) -> Result<OverwriteStatus> {
        key.push_str(ENVELOPE_SUFFIX);
        self.inner
            .put_explicit(
                ctx,
                key,
                value.into_blobstore_bytes(),
                PutBehaviour::Overwrite,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use borrowed::borrowed;
    use bytes::Bytes;
    use fbinit::FacebookInit;
    use memblob::Memblob;
    use rand::RngCore;
    use rand::SeedableRng;
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
        let is_present = inner_blobstore
            .is_present(ctx, inner_key)
            .await?
            .assume_not_found_if_unsure();
        assert!(is_present);

        // Check the key without suffix is not there
        let is_not_present = !inner_blobstore
            .is_present(ctx, outer_key)
            .await?
            .assume_not_found_if_unsure();
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
        let is_present = inner_blobstore
            .is_present(ctx, &inner_key)
            .await?
            .assume_not_found_if_unsure();
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

    #[fbinit::test]
    async fn single_precompressed_test(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);
        let innerblob = Arc::new(Memblob::default());
        let packblob = PackBlob::new(innerblob, PackFormat::ZstdIndividual(0));

        let bytes_in = Bytes::from(vec![7u8; 65535]);
        let value = BlobstoreBytes::from_bytes(bytes_in.clone());

        let key = "repo0000.compressible";
        let compressed = pack::SingleCompressed::new(19, value.clone())?;

        assert!(
            compressed.get_compressed_size()? < 65535,
            "Blob grew in compression"
        );

        packblob
            .put_single(ctx, key.to_string(), compressed)
            .await?;

        assert_eq!(
            packblob.get(ctx, key).await?.map(|b| b.into_bytes()),
            Some(value)
        );
        Ok(())
    }
}

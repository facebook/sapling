/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::envelope::PackEnvelope;
use crate::store;

use anyhow::{bail, format_err, Error, Result};
use ascii::AsciiString;
use bytes::{buf::BufExt, buf::BufMutExt, Bytes, BytesMut};
use mononoke_types::{hash::Context as HashContext, repo::REPO_PREFIX_REGEX, BlobstoreBytes};
use packblob_thrift::{
    PackedEntry, PackedFormat, PackedValue, SingleValue, StorageEnvelope, StorageFormat,
    ZstdFromDictValue,
};
use std::{
    collections::HashMap,
    io::{self, Cursor, Write},
};
use zstd::dict::EncoderDictionary;
use zstd::stream::read::Decoder as ZstdDecoder;
use zstd::stream::write::Encoder as ZstdEncoder;

/// An empty pack with no data. Cannot be uploaded, takes a dictionary blob
#[derive(Debug)]
pub struct EmptyPack(i32);

/// A pack containing multiple entries, ready to extend or upload
pub struct Pack {
    zstd_level: i32,
    dictionaries: HashMap<String, EncoderDictionary<'static>>,
    entries: Vec<PackedEntry>,
}

impl EmptyPack {
    /// Creates a new EmptyPack
    pub fn new(zstd_level: i32) -> Self {
        EmptyPack(zstd_level)
    }

    /// Adds the first blob to the empty pack
    pub fn add_base_blob(self, key: String, blob: BlobstoreBytes) -> Result<Pack> {
        let zstd_level = self.0;
        let bytes = blob.into_bytes();

        let dictionary = EncoderDictionary::copy(&bytes, zstd_level);

        let cursor = Cursor::new(&bytes);
        let compressed = zstd::encode_all(cursor, zstd_level)?;
        let data = PackedValue::Single(SingleValue::Zstd(Bytes::from(compressed)));

        let mut dictionaries = HashMap::new();
        dictionaries.insert(key.clone(), dictionary);
        let entries = vec![PackedEntry { key, data }];
        Ok(Pack {
            zstd_level,
            dictionaries,
            entries,
        })
    }
}

impl Pack {
    /// Adds another data blob to a pack, delta'd against a previous key
    pub fn add_delta_blob(
        &mut self,
        dict_key: String,
        key: String,
        blob: BlobstoreBytes,
    ) -> Result<()> {
        if self.dictionaries.contains_key(&key) {
            bail!("Key {} cannot appear in the same pack twice", key);
        }
        let zstd = {
            let dictionary = self
                .dictionaries
                .get(&dict_key)
                .ok_or_else(|| format_err!("Cannot find dictionary for blob {}", dict_key))?;

            let mut compressed_blob = BytesMut::with_capacity(blob.len());
            let writer = (&mut compressed_blob).writer();
            let mut encoder = ZstdEncoder::with_prepared_dictionary(writer, dictionary)?;

            encoder.write_all(blob.as_bytes())?;
            encoder.finish()?;
            compressed_blob.freeze()
        };
        // This uses `blob` (raw data) to create a dictionary that improves compression
        // at the expense of requiring the decompressor to find blob before it can
        // decompress the resulting blob
        let dictionary = EncoderDictionary::copy(blob.as_bytes(), self.zstd_level);
        let data = PackedValue::ZstdFromDict(ZstdFromDictValue { dict_key, zstd });
        self.dictionaries.insert(key.clone(), dictionary);
        self.entries.push(PackedEntry { key, data });
        Ok(())
    }

    /// Returns the compressed size of the pack contents, minus framing overheads
    pub fn get_compressed_size(&self) -> usize {
        self.entries
            .iter()
            .fold(0, |size, entry| size + get_entry_compressed_size(entry))
    }

    /// Converts the pack into something that can go into a blobstore
    pub(crate) fn into_blobstore_bytes(
        self,
        prefix: String,
    ) -> Result<(String, Vec<String>, BlobstoreBytes)> {
        for entry in &self.entries {
            if let Some(prefix) = REPO_PREFIX_REGEX.find(&entry.key) {
                bail!(
                    "Repo prefix {} found in packed blob key {}",
                    prefix.as_str(),
                    entry.key
                );
            }
        }

        let link_keys: Vec<String> = self.entries.iter().map(|entry| entry.key.clone()).collect();

        // Build the pack identity
        let mut pack_key = prefix;
        pack_key.push_str(&compute_pack_hash(&self.entries).to_string());
        pack_key.push_str(store::ENVELOPE_SUFFIX);

        let pack = PackedFormat {
            key: pack_key.clone(),
            entries: self.entries,
        };

        // Wrap in thrift encoding and returm as bytes
        Ok((
            pack_key,
            link_keys,
            PackEnvelope(StorageEnvelope {
                storage: StorageFormat::Packed(pack),
            })
            .into(),
        ))
    }
}

// Not to be used with a PackedEntry loaded from a blobstore - panics instead of handling errors
fn get_entry_compressed_size(entry: &PackedEntry) -> usize {
    match &entry.data {
        PackedValue::Single(SingleValue::Raw(bytes))
        | PackedValue::Single(SingleValue::Zstd(bytes)) => bytes.len(),
        PackedValue::ZstdFromDict(ZstdFromDictValue { zstd, .. }) => zstd.len(),
        // Can't happen, by construction - this only takes values created by this module
        PackedValue::Single(SingleValue::UnknownField(_)) | PackedValue::UnknownField(_) => {
            panic!("Unknown field")
        }
    }
}

pub(crate) fn decode_independent(v: SingleValue) -> Result<BlobstoreBytes> {
    match v {
        SingleValue::Raw(v) => Ok(BlobstoreBytes::from_bytes(v)),
        SingleValue::Zstd(v) => Ok(zstd::decode_all(v.reader()).map(BlobstoreBytes::from_bytes)?),
        SingleValue::UnknownField(e) => Err(format_err!("SingleValue::UnknownField {:?}", e)),
    }
}

fn decode_zstd_from_dict(
    k: &str,
    v: ZstdFromDictValue,
    dicts: &HashMap<String, BlobstoreBytes>,
) -> Result<BlobstoreBytes, Error> {
    match dicts.get(&v.dict_key) {
        Some(dict) => {
            let data = v.zstd.reader();
            let mut decoder = ZstdDecoder::with_dictionary(data, dict.as_bytes())?;
            let mut output_bytes = BytesMut::new();
            let mut writer = (&mut output_bytes).writer();
            io::copy(&mut decoder, &mut writer)?;

            Ok(BlobstoreBytes::from_bytes(output_bytes))
        }
        None => Err(format_err!(
            "Dictionary {} not found for key {}",
            v.dict_key,
            k
        )),
    }
}

// Unpack `key` from `packed`
pub(crate) fn decode_pack(packed: PackedFormat, key: &str) -> Result<BlobstoreBytes> {
    // Strip repo prefix, if any
    let key = match REPO_PREFIX_REGEX.find(key) {
        Some(m) => &key[m.end()..],
        None => key,
    };

    let PackedFormat {
        key: pack_key,
        entries: pack_entries,
    } = packed;

    let mut entry_map = HashMap::new();
    for entry in pack_entries {
        entry_map.insert(entry.key, entry.data);
        if entry_map.contains_key(key) {
            // Dictionaries must come before their users, so we don't care about the rest of the pack
            break;
        }
    }
    // Decode time
    let mut decoded_blobs = HashMap::new();
    let mut keys_to_decode = vec![key.to_string()];
    while let Some(next_key) = keys_to_decode.pop() {
        match entry_map.remove(dbg!(&next_key)) {
            None => {
                if next_key == key {
                    // Handled below
                    break;
                }
                return Err(format_err!(
                    "Key {} needs dictionary {} but it is not in the pack",
                    key,
                    next_key
                ));
            }
            Some(PackedValue::UnknownField(e)) => {
                return Err(format_err!("PackedValue::UnknownField {:?}", e));
            }
            Some(PackedValue::Single(v)) => {
                decoded_blobs.insert(next_key, decode_independent(v)?);
            }
            Some(PackedValue::ZstdFromDict(v)) => {
                if decoded_blobs.contains_key(&v.dict_key) {
                    let decoded = decode_zstd_from_dict(&next_key, v, &decoded_blobs)?;
                    decoded_blobs.insert(next_key, decoded);
                } else {
                    // Can't yet decode it - push the keys we need to decode this onto the work queue in order.
                    keys_to_decode.push(next_key.clone());
                    keys_to_decode.push(v.dict_key.clone());
                    // And reinsert this for the next loop
                    entry_map.insert(next_key, PackedValue::ZstdFromDict(v));
                }
            }
        }
    }
    decoded_blobs
        .remove(key)
        .ok_or_else(|| format_err!("Key {} not in the pack it is pointing to {}", key, pack_key))
}

// Hash the keys as they themselves are hashes
fn compute_pack_hash(entries: &[PackedEntry]) -> AsciiString {
    let mut hash_context = HashContext::new(b"pack");
    for entry in entries {
        hash_context.update(entry.key.len().to_le_bytes());
        hash_context.update(entry.key.as_bytes());
    }
    hash_context.finish().to_hex()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use rand::{Rng, RngCore, SeedableRng};
    use rand_xorshift::XorShiftRng;
    use std::convert::TryInto;

    #[test]
    fn decode_independent_zstd_test() -> Result<()> {
        // Highly compressable!
        let bytes_in = vec![7u8; 65535];

        // Prepare a compressed blob
        let input = Cursor::new(bytes_in.clone());
        let bytes = Bytes::from(zstd::encode_all(input, 0 /* default */)?);
        assert!(bytes.len() < bytes_in.len());

        // Test the decoder
        let decoded = decode_independent(SingleValue::Zstd(bytes))?;
        assert_eq!(decoded.as_bytes(), &Bytes::from(bytes_in));

        Ok(())
    }

    #[test]
    fn decode_zstd_from_dict_test() -> Result<()> {
        let mut rng = XorShiftRng::seed_from_u64(0); // reproducable Rng

        // Some partially incompressible data as base version
        let mut base_version = vec![7u8; 65535];
        rng.fill_bytes(&mut base_version[0..60000]);

        // Extend it with incompressible new data
        let mut next_version = base_version.clone();
        rng.fill_bytes(&mut next_version[60000..]);

        let diff = {
            let mut compressed_blob = BytesMut::new();
            let writer = (&mut compressed_blob).writer();
            let mut encoder = ZstdEncoder::with_dictionary(writer, 0, &base_version)?;

            encoder.write_all(&next_version)?;
            encoder.finish()?;
            compressed_blob.freeze()
        };

        let base_key = "base".to_string();

        let mut dicts = HashMap::new();
        dicts.insert(base_key.clone(), BlobstoreBytes::from_bytes(base_version));

        // Test the decoder
        let decoded = decode_zstd_from_dict(
            "appkey",
            ZstdFromDictValue {
                dict_key: base_key,
                zstd: diff,
            },
            &dicts,
        )?;
        assert_eq!(decoded.as_bytes(), &Bytes::from(next_version));

        Ok(())
    }

    #[test]
    fn pack_zstd_from_dict_chain_test() -> Result<()> {
        let mut rng = XorShiftRng::seed_from_u64(0); // reproducable Rng

        let mut raw_data = vec![];
        let pack = EmptyPack::new(0);

        // Some partially compressible data as base version
        let mut base_version = vec![7u8; 65535];
        rng.fill_bytes(&mut base_version[0..30000]);
        let base_key = "0".to_string();
        raw_data.push(base_version.clone());
        let mut pack =
            pack.add_base_blob(base_key, BlobstoreBytes::from_bytes(base_version.clone()))?;

        // incrementally build a pack from seeded random data
        let mut prev_version = base_version;
        for i in 1..20 {
            let mut this_version = prev_version;
            let start = 30000 + i * 1000;
            let end = start + 1000;
            rng.fill(&mut this_version[start..end]);
            raw_data.push(this_version.clone());
            prev_version = this_version.clone();
            pack.add_delta_blob(
                (i - 1).to_string(),
                i.to_string(),
                BlobstoreBytes::from_bytes(this_version),
            )?;
        }

        let (key, links, blob) = pack.into_blobstore_bytes(String::new())?;

        for (name, i) in links.into_iter().zip(0..20) {
            assert_eq!(i.to_string(), name);
        }

        let packed = {
            let envelope: PackEnvelope = blob.try_into()?;
            if let StorageFormat::Packed(pack) = envelope.0.storage {
                pack
            } else {
                bail!("Packing resulted in a single value, not a pack");
            }
        };

        assert_eq!(packed.key, key);

        // Check for any change of hashing approach
        assert_eq!(
            "f551ebbe66cfb504befd393604c53af5270b98d50fd7b1bf2d2aa3814c80e325.pack",
            packed.key
        );

        // Test reads roundtrip back to the raw form
        for (raw_data, i) in raw_data.into_iter().zip(0..20) {
            let value = decode_pack(packed.clone(), &i.to_string())?;
            assert_eq!(value.into_bytes(), Bytes::from(raw_data));
        }

        Ok(())
    }

    #[test]
    fn pack_zstd_from_dict_unchained_test() -> Result<()> {
        let mut rng = XorShiftRng::seed_from_u64(0); // reproducable Rng

        let mut raw_data = vec![];
        let pack = EmptyPack::new(0);

        // Some partially compressible data as base version
        let mut base_version = vec![7u8; 65535];
        rng.fill_bytes(&mut base_version[0..30000]);
        let base_key = "0".to_string();
        raw_data.push(base_version.clone());
        let mut pack =
            pack.add_base_blob(base_key, BlobstoreBytes::from_bytes(base_version.clone()))?;

        // incrementally build a pack from seeded random data
        let mut prev_version = base_version;
        for i in 1..20 {
            let mut this_version = prev_version;
            let start = 30000 + i * 1000;
            let end = start + 1000;
            rng.fill(&mut this_version[start..end]);
            raw_data.push(this_version.clone());
            prev_version = this_version.clone();
            pack.add_delta_blob(
                "0".to_string(),
                i.to_string(),
                BlobstoreBytes::from_bytes(this_version),
            )?;
        }

        let (key, links, blob) = pack.into_blobstore_bytes(String::new())?;

        for (name, i) in links.into_iter().zip(0..20) {
            assert_eq!(i.to_string(), name);
        }

        let packed = {
            let envelope: PackEnvelope = blob.try_into()?;
            if let StorageFormat::Packed(pack) = envelope.0.storage {
                pack
            } else {
                bail!("Packing resulted in a single value, not a pack");
            }
        };

        assert_eq!(packed.key, key);

        // Check for any change of hashing approach
        assert_eq!(
            "f551ebbe66cfb504befd393604c53af5270b98d50fd7b1bf2d2aa3814c80e325.pack",
            packed.key
        );

        // Test reads roundtrip back to the raw form
        for (raw_data, i) in raw_data.into_iter().zip(0..20) {
            let value = decode_pack(packed.clone(), &i.to_string())?;
            assert_eq!(value.into_bytes(), Bytes::from(raw_data));
        }

        Ok(())
    }

    #[test]
    fn pack_size_test() -> Result<()> {
        let mut rng = XorShiftRng::seed_from_u64(0); // reproducable Rng

        let mut raw_data = vec![];
        let pack = EmptyPack::new(19);

        // Some partially compressible data as base version
        let mut base_version = vec![7u8; 65535];
        rng.fill_bytes(&mut base_version[0..30000]);
        let base_key = "0".to_string();
        raw_data.push(base_version.clone());
        let mut pack =
            pack.add_base_blob(base_key, BlobstoreBytes::from_bytes(base_version.clone()))?;

        // Check the compressed size is reasonable
        let base_compressed_size = pack.get_compressed_size();
        assert!(
            base_compressed_size > 1024,
            "Compression turned 64 KiB into {} bytes - suspiciously small",
            base_compressed_size
        );
        assert!(
            base_compressed_size < 65535,
            "Compression turned 64 KiB into {} bytes - expansion unexpected",
            base_compressed_size
        );

        // incrementally build a pack from seeded random data
        let mut prev_version = base_version;
        for i in 1..20 {
            let mut this_version = prev_version;
            let start = 30000 + i * 1000;
            let end = start + 1000;
            rng.fill(&mut this_version[start..end]);
            raw_data.push(this_version.clone());
            prev_version = this_version.clone();
            pack.add_delta_blob(
                (i - 1).to_string(),
                i.to_string(),
                BlobstoreBytes::from_bytes(this_version),
            )?;
        }

        // And check it's grown, but not too far given how compressible this should be.
        let compressed_size = pack.get_compressed_size();
        assert!(
            compressed_size > base_compressed_size,
            "Pack shrank as it gained data"
        );
        let limit = base_compressed_size + 20 * 1000;
        assert!(
            compressed_size < limit,
            "Pack grew by more than the size of added data. Expected {} < {}",
            compressed_size,
            limit,
        );

        Ok(())
    }
}

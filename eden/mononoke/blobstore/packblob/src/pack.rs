/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::store;

use anyhow::{format_err, Error};
use ascii::AsciiString;
use blobstore::{BlobstoreGetData, BlobstoreMetadata};
use bytes::Bytes;
use mononoke_types::{hash::Context as HashContext, repo::REPO_PREFIX_REGEX, BlobstoreBytes};
use packblob_thrift::{PackedEntry, PackedFormat, PackedValue, SingleValue, ZstdFromDictValue};
use std::{collections::HashMap, io::Cursor};

pub fn decode_independent(
    meta: BlobstoreMetadata,
    v: SingleValue,
) -> Result<BlobstoreGetData, Error> {
    match v {
        SingleValue::Raw(v) => Ok(BlobstoreGetData::new(meta, BlobstoreBytes::from_bytes(v))),
        SingleValue::Zstd(v) => Ok(zstd::decode_all(Cursor::new(v))
            .map(|v| BlobstoreGetData::new(meta, BlobstoreBytes::from_bytes(v)))?),
        SingleValue::UnknownField(e) => Err(format_err!("SingleValue::UnknownField {:?}", e)),
    }
}

fn decode_zstd_from_dict(
    meta: BlobstoreMetadata,
    k: &str,
    v: ZstdFromDictValue,
    dicts: HashMap<String, Bytes>,
) -> Result<BlobstoreGetData, Error> {
    match dicts.get(&v.dict_key) {
        Some(dict) => {
            let v = zstdelta::apply(dict, &v.zstd)?;
            Ok(BlobstoreGetData::new(meta, BlobstoreBytes::from_bytes(v)))
        }
        None => Err(format_err!(
            "Dictionary {} not found for key {}",
            v.dict_key,
            k
        )),
    }
}

// Unpack `key` from `packed`
pub fn decode_pack(
    pack_meta: BlobstoreMetadata,
    packed: PackedFormat,
    key: String,
) -> Result<BlobstoreGetData, Error> {
    // Strip repo prefix, if any
    let key = match REPO_PREFIX_REGEX.find(&key) {
        Some(m) => String::from_utf8(key[m.end()..].as_bytes().to_vec())?,
        None => key,
    };

    let mut possible_dicts = HashMap::new();
    let mut remaining_entries = vec![];
    for entry in packed.entries {
        let current_key = entry.key;
        let value = match entry.data {
            PackedValue::Single(v) => Some(decode_independent(pack_meta.clone(), v)?),
            v => {
                remaining_entries.push(PackedEntry {
                    key: current_key.clone(),
                    data: v,
                });
                None
            }
        };
        if let Some(value) = value {
            if current_key == key {
                // short circuit, desired key was not delta compressed
                return Ok(value);
            }
            possible_dicts.insert(current_key, value.into_bytes().into_bytes());
        }
    }

    for entry in remaining_entries {
        if entry.key == key {
            match entry.data {
                PackedValue::Single(_v) => {
                    return Err(format_err!(
                        "Unexpected PackedValue::Single on key {}",
                        &key
                    ))
                }
                PackedValue::ZstdFromDict(v) => {
                    return Ok(decode_zstd_from_dict(pack_meta, &key, v, possible_dicts)?)
                }
                PackedValue::UnknownField(e) => {
                    return Err(format_err!("PackedValue::UnknownField {:?}", e))
                }
            }
        }
    }
    Err(format_err!(
        "Key {} not in the pack it is pointing to {}",
        key,
        packed.key
    ))
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

// Didn't call this encode, as the packer producing the entries is the real
// encoder.
pub fn create_packed(entries: Vec<PackedEntry>) -> Result<PackedFormat, Error> {
    // make sure we don't embedded repo prefixes inside a blob
    let entries: Vec<PackedEntry> = entries
        .into_iter()
        .map(|entry| match REPO_PREFIX_REGEX.find(&entry.key) {
            Some(m) => Ok(PackedEntry {
                key: String::from_utf8(entry.key[m.end()..].as_bytes().to_vec())?,
                data: entry.data,
            }),
            None => Ok(entry),
        })
        .collect::<Result<Vec<PackedEntry>, Error>>()?;

    // Build the pack identity
    let mut pack_key = compute_pack_hash(&entries).to_string();
    pack_key.push_str(store::ENVELOPE_SUFFIX);

    Ok(PackedFormat {
        key: pack_key,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use rand::{Rng, RngCore, SeedableRng};
    use rand_xorshift::XorShiftRng;

    #[test]
    fn decode_independent_zstd_test() -> Result<(), Error> {
        // Highly compressable!
        let bytes_in = vec![7u8; 65535];

        // Prepare a compressed blob
        let input = Cursor::new(bytes_in.clone());
        let bytes = zstd::encode_all(input, 0 /* default */)?;
        assert!(bytes.len() < bytes_in.len());

        // Test the decoder
        let decoded = decode_independent(BlobstoreMetadata::new(None), SingleValue::Zstd(bytes))?;
        assert_eq!(decoded.as_bytes().as_bytes(), &Bytes::from(bytes_in));

        Ok(())
    }

    #[test]
    fn decode_zstd_from_dict_test() -> Result<(), Error> {
        let mut rng = XorShiftRng::seed_from_u64(0); // reproducable Rng

        // Some partially incompressible data as base version
        let mut base_version = vec![7u8; 65535];
        rng.fill_bytes(&mut base_version[0..60000]);

        // Extend it with incompressible new data
        let mut next_version = base_version.clone();
        rng.fill_bytes(&mut next_version[60000..]);

        let diff = zstdelta::diff(&base_version, &next_version)?;

        let base_key = "base".to_string();

        let mut dicts = HashMap::new();
        dicts.insert(base_key.clone(), Bytes::from(base_version));

        // Test the decoder
        let decoded = decode_zstd_from_dict(
            BlobstoreMetadata::new(None),
            "appkey",
            ZstdFromDictValue {
                dict_key: base_key,
                zstd: diff,
            },
            dicts,
        )?;
        assert_eq!(decoded.as_bytes().as_bytes(), &Bytes::from(next_version));

        Ok(())
    }

    #[test]
    fn pack_zstd_from_dict_test() -> Result<(), Error> {
        let mut rng = XorShiftRng::seed_from_u64(0); // reproducable Rng

        let mut raw_data = vec![];
        let mut entries = vec![];

        // Some partially compressible data as base version
        let mut base_version = vec![7u8; 65535];
        rng.fill_bytes(&mut base_version[0..30000]);
        let base_key = "0".to_string();
        entries.push(PackedEntry {
            key: base_key.clone(),
            data: PackedValue::Single(SingleValue::Raw(base_version.clone())),
        });
        raw_data.push(base_version.clone());

        // incrementally build a pack from seeded random data
        let mut prev_version = base_version.clone();
        for i in 1..20 {
            let mut this_version = prev_version;
            let start = 30000 + i * 1000;
            let end = start + 1000;
            rng.fill(&mut this_version[start..end]);
            raw_data.push(this_version.clone());
            // Keep deltaing vs base version
            let diff = zstdelta::diff(&base_version, &this_version)?;
            prev_version = this_version;
            let entry = PackedEntry {
                key: i.to_string(),
                data: PackedValue::ZstdFromDict(ZstdFromDictValue {
                    dict_key: base_key.clone(),
                    zstd: diff.to_vec(),
                }),
            };
            entries.push(entry);
        }

        let packed = create_packed(entries)?;

        // Check for any change of hashing approach
        assert_eq!(
            "f551ebbe66cfb504befd393604c53af5270b98d50fd7b1bf2d2aa3814c80e325.pack",
            packed.key
        );

        // Test reads roundtrip back to the raw form
        for i in 0..20 {
            let value = decode_pack(BlobstoreMetadata::new(None), packed.clone(), i.to_string())?;
            assert_eq!(value.as_bytes().as_bytes().to_vec(), raw_data[i]);
        }

        Ok(())
    }

    #[test]
    fn pack_test() -> Result<(), Error> {
        let mut rng = XorShiftRng::seed_from_u64(0); // reproducable Rng

        // build a pack from seeded random data
        let entries: Vec<_> = (0..20)
            .map(|i| {
                let mut test_data = [0u8; 1024];
                rng.fill(&mut test_data);
                PackedEntry {
                    key: i.to_string(),
                    data: PackedValue::Single(SingleValue::Raw(test_data.to_vec())),
                }
            })
            .collect();

        let packed = create_packed(entries)?;

        // Check for any change of hashing approach
        assert_eq!(
            "f551ebbe66cfb504befd393604c53af5270b98d50fd7b1bf2d2aa3814c80e325.pack",
            packed.key
        );

        // See if we can get the data back
        let value1 = decode_pack(
            BlobstoreMetadata::new(None),
            packed.clone(),
            "1".to_string(),
        )?;
        assert_eq!(value1.as_bytes().len(), 1024);

        // See if we get error for unknown key
        let missing = decode_pack(BlobstoreMetadata::new(None), packed, "missing".to_string());
        assert!(missing.is_err());

        Ok(())
    }
}

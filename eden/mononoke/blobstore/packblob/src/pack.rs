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
use mononoke_types::{hash::Context as HashContext, repo::REPO_PREFIX_REGEX, BlobstoreBytes};
use packblob_thrift::{PackedEntry, PackedFormat, PackedValue, SingleValue};

pub fn decode_independent(
    meta: BlobstoreMetadata,
    v: SingleValue,
) -> Result<BlobstoreGetData, Error> {
    match v {
        SingleValue::Raw(v) => Ok(BlobstoreGetData::new(meta, BlobstoreBytes::from_bytes(v))),
        // TODO, handle Zstd case
        e => Err(format_err!("Unexpected SingleValue {:?}", e)),
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

    let entries = packed.entries;
    for entry in entries {
        if entry.key == key {
            match entry.data {
                PackedValue::Single(v) => return decode_independent(pack_meta, v),
                // TODO handle ZstdFromDictValue case
                e => return Err(format_err!("Unexpected PackedValue {:?}", e)),
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
    use rand::{Rng, SeedableRng};
    use rand_xorshift::XorShiftRng;

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

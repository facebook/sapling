/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Types and helper functions to convert a `KeyId` to a Key (`[u8]`). The radix index is
//! responsible for the other direction - `[u8]` to `KeyId`. This mapping is to remove the
//! need of storing full keys in the radix index.
//!
//! Typically, there is an append-only file which is the source of truth of some information.
//! The `KeyId` could be offsets in that file.
//!
//! The radix index APIs only care about how to read a key, given a `KeyId`. It does not
//! care about how to write a key. This is more flexible. For example, the source of truth
//! file may have keys stored already before the index gets built. The `append` methods provided
//! below are only for convenience.
//!
//! There could be cases where the `KeyId`s are not offsets, or the keys are not stored as-is.
//! For example, `KeyId` could be mercurial revision numbers instead of offsets for non-inlined
//! revlog .i files. The radix index has a limitation (to make the format slightly more compact)
//! that one key cannot be the prefix of another different key. This could be guaranteed by using
//! the fixed length key, or appending `'\0'` to strings that cannot have `'\0'`. But in case
//! of variant-length binary key, some escaping is needed to avoid the prefix conflict. The
//! escaping logic might be implemented as key read/write functions.

use crate::errors::ErrorKind;
use crate::traits::Resize;
use anyhow::{bail, Result};
use std::io::{Cursor, Seek, SeekFrom, Write};
use vlqencoding::{VLQDecode, VLQEncode};

/// Integer that maps to a key (`[u8]`).
#[derive(Debug, Eq, PartialEq, PartialOrd, Ord, Clone, Copy, Default)]
pub struct KeyId(u32);

macro_rules! impl_convert {
    ($T: ty) => {
        impl From<$T> for KeyId {
            #[allow(unused_comparisons)]
            #[inline]
            fn from(v: $T) -> Self {
                if v > 0xffff_ffff || v < 0 {
                    panic!("KeyId out of range")
                }
                KeyId(v as u32)
            }
        }

        impl Into<$T> for KeyId {
            #[inline]
            fn into(self) -> $T {
                self.0 as $T
            }
        }
    };
}

impl_convert!(u32);
impl_convert!(u64);

// Make sure `usize` contains at least 4 bytes (`KeyId` size).
const _SIZE_TEST: usize = 0xffff_ffff;
impl_convert!(usize);

/// Keys with fixed 20 bytes length.
pub struct FixedKey;

/// Keys with variant-length. Serialized as a VLQ-encoded length, followed by the actual key.
pub struct VariantKey;

impl FixedKey {
    #[inline]
    pub fn read<'a, K: AsRef<[u8]>>(key_buf: &'a K, key_id: KeyId) -> Result<&'a [u8]> {
        let key_buf = key_buf.as_ref();
        let start_pos: usize = key_id.into();
        // LANG: Consider making 20 a type parameter once supported.
        let end_pos = start_pos + 20;
        if key_buf.len() < end_pos {
            bail!(ErrorKind::InvalidKeyId(key_id));
        }
        Ok(&key_buf[start_pos..end_pos])
    }

    #[inline]
    pub fn append<B: AsMut<[u8]> + Resize<u8>, K: AsRef<[u8]>>(key_buf: &mut B, key: &K) -> KeyId {
        let key = key.as_ref();
        assert_eq!(key.len(), 20);

        let pos = key_buf.as_mut().len();
        key_buf.resize(pos + 20, 0);
        let key_buf = key_buf.as_mut();
        key_buf[pos..pos + 20].copy_from_slice(key);
        pos.into()
    }
}

impl VariantKey {
    #[inline]
    pub fn read<'a, K: AsRef<[u8]>>(key_buf: &'a K, key_id: KeyId) -> Result<&'a [u8]> {
        let key_buf = key_buf.as_ref();
        let mut reader = Cursor::new(key_buf);
        reader.seek(SeekFrom::Start(key_id.into()))?;
        let key_len: usize = reader.read_vlq()?;

        let start_pos = reader.seek(SeekFrom::Current(0))? as usize;
        let end_pos = start_pos + key_len;
        if key_buf.len() < end_pos {
            bail!(ErrorKind::InvalidKeyId(key_id))
        }
        Ok(&key_buf[start_pos as usize..end_pos as usize])
    }

    #[inline]
    pub fn append<B: AsMut<[u8]> + Resize<u8>, K: AsRef<[u8]>>(key_buf: &mut B, key: &K) -> KeyId {
        let key = key.as_ref();
        // PERF: the Vec allocation may be avoided with a more complex implementation.
        // Most of the time, key length could be encoded in 2-byte VLQ. Pre-allocate 2 bytes.
        let mut buf = Vec::<u8>::with_capacity(key.len() + 2);
        buf.write_vlq(key.len()).expect("write len");
        buf.write_all(key).expect("write key");

        let pos = key_buf.as_mut().len();
        key_buf.resize(pos + buf.len(), 0);
        let key_buf = key_buf.as_mut();
        key_buf[pos..pos + buf.len()].copy_from_slice(&buf[..]);
        (pos as u64).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rand::{distributions::Alphanumeric, thread_rng, Rng, RngCore};

    #[test]
    fn test_variant_key_round_trip() {
        let rng = thread_rng();
        let mut buf = Vec::<u8>::new();
        let keys: Vec<Vec<u8>> = (0..1000usize)
            .map(|i| {
                rng.sample_iter(&Alphanumeric)
                    .take(i % 40)
                    .map(|ch| ch as u8)
                    .collect()
            })
            .collect();
        let key_ids: Vec<KeyId> = keys
            .iter()
            .map(|key| VariantKey::append(&mut buf, key))
            .collect();
        let keys_retrieved: Vec<Vec<u8>> = key_ids
            .iter()
            .map(|&id| VariantKey::read(&buf, id).unwrap().to_vec())
            .collect();
        assert_eq!(keys, keys_retrieved);
    }

    #[test]
    fn test_fixed_key_round_trip() {
        let mut rng = thread_rng();
        let mut buf = Vec::<u8>::new();
        let keys: Vec<[u8; 20]> = (0..1000usize)
            .map(|_| {
                let mut bytes = [0u8; 20];
                rng.fill_bytes(&mut bytes);
                bytes
            })
            .collect();
        let key_ids: Vec<KeyId> = keys
            .iter()
            .map(|key| FixedKey::append(&mut buf, key))
            .collect();
        assert_eq!(buf.len(), 1000 * 20);
        let keys_retrieved: Vec<[u8; 20]> = key_ids
            .iter()
            .map(|&id| {
                let mut bytes = [0u8; 20];
                bytes.copy_from_slice(FixedKey::read(&buf, id).unwrap());
                bytes
            })
            .collect();
        assert_eq!(keys, keys_retrieved);
    }
}

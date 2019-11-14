/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Main radix index implementation that maintains efficient Key to `KeyId` look ups.
//!
//! Practically, the index usually requires 2 buffers to be fully functional:
//!
//!  - An key buffer. It stores the actual key contents. It is usually an
//!    append-only buffer.
//!  - A radix buffer. It stores radix nodes and pointers (offsets) to the key
//!    buffer. It does not contain key contents. For operations that requires
//!    contents of keys (ex. looking up unknown keys; inserting keys), the key
//!    buffer and a function to convert `KeyId` (offset) to key content must
//!    be provided.
//!
//! A radix node consists of 16 "pointer"s (since we follow base-16 sequence
//! to do lookups). A pointer could be one of the following:
//!
//!  - Empty (0).
//!  - A `RadixOffset`. Pointing to another radix node. (LSB is 0)
//!  - A `KeyId`. Need to be resolved by an external-provided "key function".
//!    Usually the "key function" uses `KeyId` as an offset of the key buffer.
//!    (LSB is 1).
//!
//! The radix buffer could have multiple "root" radix nodes so it contains multiple
//! distinct indexes.
//!
//! A "key function" takes a `KeyId` and a "key argument" (usually the "key buffer")
//! and returns a slice reference in the "key argument" as a key. That means, full
//! key contents usually need to be written down in the key buffer, instead of going
//! through extra pre-processing logic. That said, the "key argument" does not always
//! have to be a "key buffer" so there could be some flexibility here.
//!
//! To give an more detailed example, suppose the "key function" is `FixedKey::read`
//! (fixed 20-byte keys) and the key buffer ([u8]) looks like:
//!
//! ```plain,ignore
//!     Offset Content
//!     0x100: 0x12 0x34 0x56 .... (key)
//!     0x114: 0x12 0x78 0x9a .... (another key)
//! ```
//!
//! With both of the keys inserted at radix offset 0x400, the radix buffer ([u32])
//! looks like:
//!
//! ```plain,ignore
//!     Offset Content
//!     0x400: 0 0x880 0 0 0 0 0 0 0 0 0 0 0 0 0 0 (raidx node, 16 pointers)
//!              ^^^^^
//!              0x1: RadixOffset 0x440
//!     0x440: 0 0 0x900 0 0 0 0 0 0 0 0 0 0 0 0 0 (another radix node)
//!                ^^^^^
//!                0x2: RadixOffset 0x480
//!     0x480: 0 0 0 0x201 0 0 0 0x229 0 0 0 0 0 0 0 0 (another radix node)
//!                  ^^^^^       ^^^^^
//!        0x3: KeyId 0x100      0x7: KeyId 0x114
//! ```
//!
//! Note the radix buffer does not contain full key contents (ex. it does not
//! have 0x34 0x56 or 0x78 0x9a). It only has the ambiguous prefix (0x12) stored.
//!
//! The index does not support deletion or iteration at present. It also forbids
//! a key being the prefix of another key, to make the format simpler and more
//! compact.
//!
//! Extra flexibility could be achieved by making the "key buffer" include
//! additional information. For example, instead of just storing plain, fixed
//! 20-byte keys one after another, a "key entry" could be
//! "20-byte key + 4-byte offset + ..." so those key entries could contain
//! additional data.

use crate::base16::Base16Iter;
use crate::errors::ErrorKind;
use crate::key::KeyId;
use crate::traits::Resize;
use failure::{bail, Fallible as Result};

/// Number of children ("pointer"s) a radix node has
pub const RADIX_NCHILDREN: usize = 16;

/// Represent an offset to a radix node which contains 16 optional pointers to other
/// radix nodes, or `KeyId`s.
#[derive(Clone, Copy)]
struct RadixOffset(u32);

impl RadixOffset {
    #[inline]
    pub fn new(offset: u32) -> Self {
        RadixOffset(offset)
    }

    /// Append an empty `RadixNode` (`[u32; 16]`) at the end of a buffer.
    #[inline]
    pub fn create<R: Resize<u32> + AsRef<[u32]>>(vec: &mut R) -> Result<Self> {
        let pos = vec.as_ref().len();
        if (pos as u32) as usize != pos {
            bail!(ErrorKind::OffsetOverflow(pos as u64));
        }
        vec.resize(pos + RADIX_NCHILDREN, 0);
        Ok(RadixOffset(pos as u32))
    }

    /// Follow a base16 sequence. Return a tuple:
    ///   - The first item is `Some(key_id)` if a `KeyId` was found, or `None`
    ///   - The second item the "last follow state", useful for write operations
    ///
    /// The "last follow state" consists of 3 items:
    ///   - r: `RadixOffset`
    ///   - i: position reached within `seq`
    ///   - b: last base16 index number
    ///
    /// So `buf[r.0 + b]` points to the returned `KeyId` (if returned), and
    /// `seq.nth(i)` equals to `b`.
    ///
    /// For example, given the base16 sequence: [1, 2, 11, 12, 13, 14], and the
    /// following radix buffer:
    ///
    ///   - Offset   0: RadixNode({0: 100, 1: 0, ... 15: 0}) # This RadixNode
    ///   - Offset 100: RadixNode({..., 2: 200, ...})
    ///   - Offset 200: RadixNode({..., 11: 501, ...}) # 501 is `KeyId` since its LSB is 1
    ///
    /// This function will return `Ok(Some(501), (200, 2, 11))`. Note: the remaining
    /// part of the base16 sequence (starting from 12) are not verified against the
    /// key. It's up to the caller to verify it if needed.
    #[inline]
    pub fn follow<R: AsRef<[u32]>, I: Iterator<Item = u8>>(
        self,
        buf: &R,
        seq: I,
    ) -> Result<(Option<KeyId>, (RadixOffset, usize, u8))> {
        let buf = buf.as_ref();
        let mut radix = self;
        for (i, b) in seq.enumerate() {
            if b >= RADIX_NCHILDREN as u8 {
                bail!(ErrorKind::InvalidBase16(b));
            }

            let pos = radix.0 as usize + usize::from(b);
            if pos >= buf.len() {
                bail!(ErrorKind::OffsetOverflow(pos as u64));
            }

            let v = u32::from_be(buf[pos]);
            if v == 0 {
                // Missing
                return Ok((None, (radix, i, b)));
            } else if v & 1 != 0 {
                // KeyId
                return Ok((Some(KeyId::from(v >> 1)), (radix, i, b)));
            } else {
                // RadixOffset
                radix = RadixOffset::new(v >> 1);
            }
        }

        // The base16 sequence is too short and does not match a non-radix node.
        // NOTE: The error is not accurate if the prefix is empty and the radix tree is
        // also empty, or has exactly one entry. But without supporting that, the code
        // becomes much shorter. Since that is a rare case, we do not support it for now.
        Err(ErrorKind::AmbiguousPrefix.into())
    }

    /// Rewrite specified entry to point to another radix node.
    #[inline]
    pub fn write_radix<R: AsMut<[u32]>>(
        &self,
        vec: &mut R,
        index: u8,
        node: RadixOffset,
    ) -> Result<()> {
        if node.0 > 0x7fff_ffff {
            bail!(ErrorKind::OffsetOverflow(node.0 as u64));
        }
        self.write_raw(vec, index, node.0 << 1)
    }

    /// Rewrite specified entry to point to a `KeyId`.
    #[inline]
    pub fn write_key_id<R: AsMut<[u32]>>(
        &self,
        vec: &mut R,
        index: u8,
        key_id: KeyId,
    ) -> Result<()> {
        let id: u32 = key_id.into();
        if id > 0x7fff_ffff {
            bail!(ErrorKind::OffsetOverflow(key_id.into()));
        }
        self.write_raw(vec, index, (id << 1) | 1)
    }

    #[inline]
    fn write_raw<R: AsMut<[u32]>>(&self, vec: &mut R, index: u8, value: u32) -> Result<()> {
        debug_assert!(index < RADIX_NCHILDREN as u8);
        let vec = vec.as_mut();
        let pos = self.0 as usize + usize::from(index);
        if pos > vec.len() {
            bail!(ErrorKind::OffsetOverflow(pos as u64));
        }
        vec[pos] = value.to_be();
        Ok(())
    }
}

// Public APIs

/// Look up a given `Key`. Return an optional potentially matched `KeyId`.
/// `radix_buf` is a `[u32]` buffer that contains `RaidxNode`s.
/// `offset` is the offset of the root radix node within the radix buffer.
/// `key` is a base256 sequence.
/// The caller is responsible to check whether `KeyId` matches the given `Key` or not.
#[inline]
pub fn radix_lookup_unchecked<R, K>(radix_buf: &R, offset: u32, key: &K) -> Result<Option<KeyId>>
where
    R: AsRef<[u32]>,
    K: AsRef<[u8]>,
{
    let (key_id, _) = RadixOffset::new(offset).follow(radix_buf, Base16Iter::from_bin(&key))?;
    Ok(key_id)
}

// unfortunately rustfmt makes the parameter list longer than 100 chars so it's disabled for now.

/// Lookup a given `Key`. Return a verified `KeyId` or `None`.
/// `radix_buf` is a `[u32]` buffer that contains `RaidxNode`s.
/// `offset` is the offset of the root radix node within the radix buffer.
/// `key` is a base256 sequence.
/// `key_reader` and `key_reader_arg` decide how and where to read a key given a `KeyId`.
/// Unlike `radix_lookup_unchecked`. This function reads and checks the key.
#[cfg_attr(rustfmt, rustfmt_skip)]
pub fn radix_lookup<R, K, KR, KA>(
    radix_buf: &R, offset: u32, key: &K, key_reader: KR, key_reader_arg: &KA)
    -> Result<Option<KeyId>>
where
    R: AsRef<[u32]>,
    K: AsRef<[u8]>,
    KR: Fn(&KA, KeyId) -> Result<&[u8]>,
{
    let key_id = radix_lookup_unchecked(radix_buf, offset, key)?;
    if let Some(id) = key_id {
        let existing_key = key_reader(key_reader_arg, id)?;
        if existing_key != key.as_ref() {
            return Ok(None);
        }
    }
    Ok(key_id)
}

/// Lookup a unique `KeyId` given a prefix of a binary base16 sequence.
/// `radix_buf` is a `[u32]` buffer that contains `RaidxNode`s.
/// `offset` is the offset of the root radix node within the radix buffer.
/// `prefix` is a base16 sequence (not base256).
/// `key_reader` and `key_reader_arg` decide how and where to read a key given a `KeyId`.
///
/// Return `Err(ErrorKind::AmbiguousPrefix.into())` or `Err(ErrorKind::PrefixConflict.into())`
/// if there are multiple matches, or `prefix` is empty. Return `Ok(None)` if there
/// are no matches.
///
/// Return `Ok(key_id)` if there is a unique match. The `key_id` is guarnateed
/// that once resolved and converted to base16 sequence, has a prefix matching
/// the given `prefix`.
#[cfg_attr(rustfmt, rustfmt_skip)]
pub fn radix_prefix_lookup<R, P, KR, KA>(
    radix_buf: &R, offset: u32, prefix: P, key_reader: KR, key_reader_arg: &KA)
    -> Result<Option<KeyId>>
where
    R: AsRef<[u32]>,
    P: Iterator<Item = u8> + Clone,
    KR: Fn(&KA, KeyId) -> Result<&[u8]>,
{
    let root = RadixOffset::new(offset);
    let (key_id, (_radix, i, _b)) = root.follow(radix_buf, prefix.clone())?;
    if let Some(id) = key_id {
        let key = key_reader(key_reader_arg, id)?;
        let iter = Base16Iter::from_bin(&key);
        let matched = iter.clone().skip(i).zip(prefix.clone().skip(i)).all(|(b1, b2)| b1 == b2);
        if !matched || iter.count() < prefix.count() {
            return Ok(None);
        }
    }
    Ok(key_id)
}

/// Insert a `key_id`  into the radix tree that can be retrieved using its corresponding
/// key afterwards.
///
/// `radix_buf` is a `[u32]` buffer that contains `RaidxNode`s.
/// `offset` is the offset of the root radix node within the radix buffer.
/// `key_id` is the `KeyId`, which will be passed to `key_reader` to retrieve the actual key.
/// `key_reader` and `key_reader_arg` decide how and where to read a key given a `KeyId`.
///
/// Return `Ok(())` on success.
///
/// The key being inserted can neither be a prefix of an existing key, or has a prefix that equals
/// to an existing key. If the key already exists, `key_id` must match the existing `key_id`.
/// Otherwise it will cause `ErrorKind::PrefixConflict` error.
#[cfg_attr(rustfmt, rustfmt_skip)]
pub fn radix_insert<R, KR, KA>(
    radix_buf: &mut R, offset: u32, key_id: KeyId, key_reader: KR, key_reader_arg: &KA)
    -> Result<()>
where
    R: Resize<u32> + AsRef<[u32]> + AsMut<[u32]>,
    KR: Fn(&KA, KeyId) -> Result<&[u8]>,
{
    let new_key = key_reader(key_reader_arg, key_id)?;
    radix_insert_with_key(
        radix_buf,
        offset,
        key_id,
        &new_key,
        key_reader,
        key_reader_arg,
    )
}

/// Insert a `key_id`  into the radix tree that can be retrieved using `key` afterwards.
///
/// `radix_buf` is a `[u32]` buffer that contains `RaidxNode`s.
/// `offset` is the offset of the root radix node within the radix buffer.
/// `key_id` is the `KeyId` to insert.
/// `key` is the `Key` to be used. It must match provided `key_id`.
/// `key_reader` and `key_reader_arg` decide how and where to read a key given a `KeyId`.
///
/// Return `Ok(())` on success.
///
/// The key being inserted can neither be a prefix of an existing key, or has a prefix that equals
/// to an existing key. If the key already exists, `key_id` must match the existing `key_id`.
/// Otherwise it will cause `ErrorKind::PrefixConflict` error.
#[cfg_attr(rustfmt, rustfmt_skip)]
pub fn radix_insert_with_key<R, K, KR, KA>(
    radix_buf: &mut R, offset: u32, key_id: KeyId, key: &K, key_reader: KR, key_reader_arg: &KA)
    -> Result<()>
where
    R: Resize<u32> + AsRef<[u32]> + AsMut<[u32]>,
    K: AsRef<[u8]>,
    KR: Fn(&KA, KeyId) -> Result<&[u8]>,
{
    let new_key_id = key_id;
    let new_key = key;
    let root = RadixOffset::new(offset);
    let (old_key_id, (radix, i, b)) = root.follow(radix_buf, Base16Iter::from_bin(new_key))?;
    match old_key_id {
        Some(old_key_id) => {
            // No need to re-insert a same key
            if old_key_id == new_key_id {
                return Ok(());
            }

            // Need to do a leaf split
            let old_key = key_reader(key_reader_arg, old_key_id)?;

            // Find common prefix starting from the next base16 integer
            let mut common_len = 0;
            let old_iter = Base16Iter::from_bin(&old_key).skip(i + 1);
            let new_iter = Base16Iter::from_bin(new_key).skip(i + 1);
            for (b1, b2) in old_iter.zip(new_iter) {
                if b1 == b2 {
                    common_len += 1;
                } else {
                    // Got a chain of radix nodes to write back
                    // Write new `RadixNode`s in reversed order so:
                    // - Looking up `old_key` works in the mean time
                    // - There won't be invalid `RadixOffset` at any time
                    // - Write count is optimized
                    //   (won't write `KeyId` first and then change it to `RadixOffset`)
                    // The first two properties could help concurrent reads.
                    // Although we are not depending on that right now.
                    let mut node = RadixOffset::create(radix_buf)?;
                    node.write_key_id(radix_buf, b1, old_key_id)?;
                    node.write_key_id(radix_buf, b2, new_key_id)?;
                    let new_iter = Base16Iter::from_bin(new_key).skip(i + 1);
                    for k in new_iter.take(common_len).rev() {
                        let new_node = RadixOffset::create(radix_buf)?;
                        new_node.write_radix(radix_buf, k, node)?;
                        node = new_node;
                    }
                    return radix.write_radix(radix_buf, b, node);
                }
            }

            // new_key is a prefix of old_key, or vice-versa.
            // or they are the same but with different key_ids.
            if old_key.len() > new_key.as_ref().len() {
                Err(ErrorKind::PrefixConflict(new_key_id, old_key_id).into())
            } else {
                Err(ErrorKind::PrefixConflict(old_key_id, new_key_id).into())
            }
        }
        None => radix.write_key_id(radix_buf, b, new_key_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::{FixedKey, VariantKey};
    use failure::AsFail;
    use quickcheck::quickcheck;
    use std::collections::HashSet;
    use std::mem::transmute;

    #[test]
    fn test_errors() {
        let mut key_buf = vec![0u8; 10];
        let mut radix_buf = vec![0u32; 15];

        // KeyId exceeds format limit
        let key = [0u8; 20];
        let key_id = (1u32 << 31).into();
        let r = radix_insert_with_key(&mut radix_buf, 0, key_id, &key, FixedKey::read, &key_buf);
        let t = format!("{}", r.unwrap_err());
        assert_eq!(
            t,
            format!("{}", ErrorKind::OffsetOverflow(2147483648).as_fail())
        );

        // KeyId exceeds key buffer length
        let key_id = 30u32.into();
        let r = radix_insert(&mut radix_buf, 0, key_id, FixedKey::read, &key_buf);
        let t = format!("{}", r.unwrap_err());
        assert_eq!(t, format!("{}", ErrorKind::InvalidKeyId(key_id).as_fail()));

        // Radix root node offset exceeds radix buffer length
        let r = radix_insert_with_key(&mut radix_buf, 16, key_id, &key, FixedKey::read, &key_buf);
        let t = format!("{}", r.unwrap_err());
        assert_eq!(t, format!("{}", ErrorKind::OffsetOverflow(16).as_fail()));

        // Radix node offset out of range during a lookup
        let prefix = [0xf].iter().cloned();
        let r = radix_prefix_lookup(&radix_buf, 0, prefix, FixedKey::read, &key_buf);
        let t = format!("{}", r.unwrap_err());
        assert_eq!(t, format!("{}", ErrorKind::OffsetOverflow(15).as_fail()));

        // Base16 sequence overflow
        let prefix = [21].iter().cloned();
        let r = radix_prefix_lookup(&radix_buf, 0, prefix.clone(), FixedKey::read, &key_buf);
        let t = format!("{}", r.unwrap_err());
        assert_eq!(t, format!("{}", ErrorKind::InvalidBase16(21).as_fail()));

        // Inserting a same key with a same `KeyId` is okay
        let key_id1 = VariantKey::append(&mut key_buf, &b"ab");
        let key_id2 = VariantKey::append(&mut key_buf, &b"ab");
        radix_insert(&mut radix_buf, 0, key_id1, VariantKey::read, &key_buf).expect("insert");
        radix_insert(&mut radix_buf, 0, key_id1, VariantKey::read, &key_buf).expect("insert");

        // But not okay if `KeyId` are different
        let r = radix_insert(&mut radix_buf, 0, key_id2, VariantKey::read, &key_buf);
        let t = format!("{}", r.unwrap_err());
        assert_eq!(
            t,
            format!("{}", ErrorKind::PrefixConflict(key_id1, key_id2).as_fail())
        );

        // A key cannot be a prefix of another key
        let key_id4 = VariantKey::append(&mut key_buf, &b"a");
        let key_id5 = VariantKey::append(&mut key_buf, &b"abc");
        let r = radix_insert(&mut radix_buf, 0, key_id4, VariantKey::read, &key_buf);
        assert_eq!(
            format!("{}", r.unwrap_err()),
            format!("{}", ErrorKind::PrefixConflict(key_id4, key_id1))
        );
        let r = radix_insert(&mut radix_buf, 0, key_id5, VariantKey::read, &key_buf);
        assert_eq!(
            format!("{}", r.unwrap_err()),
            format!("{}", ErrorKind::PrefixConflict(key_id1, key_id5))
        );

        // Enforce a leaf split of key_id1
        let key_id3 = VariantKey::append(&mut key_buf, &b"ac");
        radix_insert(&mut radix_buf, 0, key_id3, VariantKey::read, &key_buf).expect("insert");

        // Still impossible to cause key prefix conflicts
        let r = radix_insert(&mut radix_buf, 0, key_id4, VariantKey::read, &key_buf);
        let t = format!("{}", r.unwrap_err());
        assert_eq!(t, format!("{}", ErrorKind::AmbiguousPrefix.as_fail()));
        let r = radix_insert(&mut radix_buf, 0, key_id5, VariantKey::read, &key_buf);
        let t = format!("{}", r.unwrap_err());
        assert_eq!(
            t,
            format!("{}", ErrorKind::PrefixConflict(key_id1, key_id5).as_fail())
        );
    }

    #[test]
    fn test_prefix_lookup() {
        let mut key_buf: Vec<u8> = vec![];
        let mut radix_buf = vec![0u32; 16];

        let query = Base16Iter::from_bin(&b"01abc");

        // With a single key
        let key1 = b"01ab";
        let key1_id = VariantKey::append(&mut key_buf, &key1);
        radix_insert(&mut radix_buf, 0, key1_id, VariantKey::read, &key_buf).expect("insert");
        for i in 0..query.len() {
            let prefix = query.clone().take(i);
            let r = radix_prefix_lookup(&radix_buf, 0, prefix, VariantKey::read, &key_buf);
            if i == 0 {
                // This is sub-optimal. But see the NOTE in RadixOffset::follow.
                let t = format!("{}", r.unwrap_err());
                assert_eq!(t, format!("{}", ErrorKind::AmbiguousPrefix.as_fail()));
            } else if i <= key1.len() * 2 {
                assert_eq!(r.unwrap(), Some(key1_id));
            } else {
                assert_eq!(r.unwrap(), None);
            }
        }

        // With another key
        let key2 = b"01bbc";
        let key2_id = VariantKey::append(&mut key_buf, &key2);
        radix_insert(&mut radix_buf, 0, key2_id, VariantKey::read, &key_buf).expect("insert");
        for i in 0..query.len() {
            let prefix = query.clone().take(i);
            let r = radix_prefix_lookup(&radix_buf, 0, prefix, VariantKey::read, &key_buf);
            if i <= 5 {
                let t = format!("{}", r.unwrap_err());
                assert_eq!(t, format!("{}", ErrorKind::AmbiguousPrefix.as_fail()));
            } else if i <= key1.len() * 2 {
                assert_eq!(r.unwrap(), Some(key1_id));
            } else {
                assert_eq!(r.unwrap(), None);
            }
        }

        let query = Base16Iter::from_bin(&b"1");
        let r = radix_prefix_lookup(&radix_buf, 0, query, VariantKey::read, &key_buf);
        assert_eq!(r.unwrap(), None);

        let query = Base16Iter::from_bin(&b"01b");
        let r = radix_prefix_lookup(&radix_buf, 0, query, VariantKey::read, &key_buf);
        assert_eq!(r.unwrap(), Some(key2_id));
    }

    quickcheck! {
        fn test_compare_with_stdset_sparse(std_set: HashSet<u64>) -> bool {
            let std_set: HashSet<[u8; 10]> = std_set.iter().map(|&x|  {
                let mut buf = [0u8; 10];
                let slice: [u8; 8] = unsafe { transmute(x) };
                buf[0..8].copy_from_slice(&slice);
                buf
            }).collect();
            check_with_stdset(std_set)
        }

        fn test_compare_with_stdset_dense(std_set: HashSet<u16>) -> bool {
            let std_set: HashSet<[u8; 10]> = std_set.iter().map(|&x|  {
                let mut buf = [0u8; 10];
                let slice: [u8; 2] = unsafe { transmute(x) };
                buf[0..2].copy_from_slice(&slice);
                buf
            }).collect();
            check_with_stdset(std_set)
        }
    }

    // Compare with `HashSet`.
    fn check_with_stdset(std_set: HashSet<[u8; 10]>) -> bool {
        let mut key_buf = Vec::<u8>::with_capacity(std_set.len() * 11);
        let mut radix_buf = vec![0u32; 16];

        // Insert to radix tree
        for key in &std_set {
            let key_id = VariantKey::append(&mut key_buf, key);
            radix_insert(&mut radix_buf, 0, key_id, VariantKey::read, &key_buf).expect("insert");
        }

        // Test key existence
        std_set.iter().all(|key| {
            let r = radix_lookup(&radix_buf, 0, key, VariantKey::read, &key_buf);
            r.unwrap().is_some()
        })
    }
}

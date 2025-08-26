/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Index support for `log`.
//!
//! See [`Index`] for the main structure.

// File format:
//
// ```plain,ignore
// INDEX       := HEADER + ENTRY_LIST
// HEADER      := '\0'  (takes offset 0, so 0 is not a valid offset for ENTRY)
// ENTRY_LIST  := RADIX | ENTRY_LIST + ENTRY
// ENTRY       := RADIX | LEAF | LINK | KEY | ROOT + REVERSED(VLQ(ROOT_LEN)) |
//                ROOT + CHECKSUM + REVERSED(VLQ(ROOT_LEN + CHECKSUM_LEN))
// RADIX       := '\2' + RADIX_FLAG (1 byte) + BITMAP (2 bytes) +
//                PTR2(RADIX | LEAF) * popcnt(BITMAP) + PTR2(LINK)
// LEAF        := '\3' + PTR(KEY | EXT_KEY) + PTR(LINK)
// LINK        := '\4' + VLQ(VALUE) + PTR(NEXT_LINK | NULL)
// KEY         := '\5' + VLQ(KEY_LEN) + KEY_BYTES
// EXT_KEY     := '\6' + VLQ(KEY_START) + VLQ(KEY_LEN)
// INLINE_LEAF := '\7' + EXT_KEY + LINK
// ROOT        := '\1' + PTR(RADIX) + VLQ(META_LEN) + META
// CHECKSUM    := '\8' + PTR(PREVIOUS_CHECKSUM) + VLQ(CHUNK_SIZE_LOGARITHM) +
//                VLQ(CHECKSUM_CHUNK_START) + XXHASH_LIST + CHECKSUM_XX32 (LE32)
// XXHASH_LIST := A list of 64-bit xxhash in Little Endian.
//
// PTR(ENTRY)  := VLQ(the offset of ENTRY)
// PTR2(ENTRY) := the offset of ENTRY, in 0 or 4, or 8 bytes depending on BITMAP and FLAGS
//
// RADIX_FLAG := USE_64_BIT (1 bit) + RESERVED (6 bits) + HAVE_LINK (1 bit)
// ```
//
// Some notes about the format:
//
// - A "RADIX" entry has 16 children. This is mainly for source control hex hashes. The "N"
//   in a radix entry could be less than 16 if some of the children are missing (ex. offset = 0).
//   The corresponding jump table bytes of missing children are 0s. If child i exists, then
//   `jumptable[i]` is the relative (to the beginning of radix entry) offset of PTR(child offset).
// - A "ROOT" entry its length recorded as the last byte. Normally the root entry is written
//   at the end. This makes it easier for the caller - it does not have to record the position
//   of the root entry. The caller could optionally provide a root location.
// - An entry has a 1 byte "type". This makes it possible to do a linear scan from the
//   beginning of the file, instead of having to go through a root. Potentially useful for
//   recovery purpose, or adding new entry types (ex. tree entries other than the 16-children
//   radix entry, value entries that are not u64 linked list, key entries that refers external
//   buffer).
// - The "EXT_KEY" type has a logically similar function with "KEY". But it refers to an external
//   buffer. This is useful to save spaces if the index is not a source of truth and keys are
//   long.
// - The "INLINE_LEAF" type is basically an inlined version of EXT_KEY and LINK, to save space.
// - The "ROOT_LEN" is reversed so it can be read byte-by-byte from the end of a file.

use std::borrow::Cow;
use std::cmp::Ordering::Equal;
use std::cmp::Ordering::Greater;
use std::cmp::Ordering::Less;
use std::fmt;
use std::fmt::Debug;
use std::fmt::Formatter;
use std::fs;
use std::fs::File;
use std::hash::Hasher;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::mem::size_of;
use std::ops::Bound;
use std::ops::Bound::Excluded;
use std::ops::Bound::Included;
use std::ops::Bound::Unbounded;
use std::ops::Deref;
use std::ops::RangeBounds;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::AcqRel;
use std::sync::atomic::Ordering::Acquire;
use std::sync::atomic::Ordering::Relaxed;

use byteorder::ByteOrder;
use byteorder::LittleEndian;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use fs2::FileExt as _;
use minibytes::Bytes;
use tracing::debug_span;
use twox_hash::XxHash;
use vlqencoding::VLQDecodeAt;
use vlqencoding::VLQEncode;

use crate::base16::Base16Iter;
use crate::base16::base16_to_base256;
use crate::base16::single_hex_to_base16;
use crate::config;
use crate::errors::IoResultExt;
use crate::errors::ResultExt;
use crate::lock::ScopedFileLock;
use crate::utils;
use crate::utils::mmap_bytes;
use crate::utils::xxhash;
use crate::utils::xxhash32;

/// Structures and serialization

#[derive(Clone, PartialEq, Default)]
struct MemRadix {
    pub offsets: [Offset; 16],
    pub link_offset: LinkOffset,
}

#[derive(Clone, PartialEq)]
struct MemLeaf {
    pub key_offset: Offset,
    pub link_offset: LinkOffset,
}

#[derive(Clone, PartialEq)]
struct MemKey {
    pub key: Box<[u8]>, // base256
}

#[derive(Clone, PartialEq)]
struct MemExtKey {
    pub start: u64,
    pub len: u64,
}

#[derive(Clone, PartialEq)]
struct MemLink {
    pub value: u64,
    pub next_link_offset: LinkOffset,
    pub unused: bool,
}

#[derive(Clone, PartialEq)]
struct MemRoot {
    pub radix_offset: RadixOffset,
    pub meta: Box<[u8]>,
}

/// A Checksum entry specifies the checksums for all bytes before the checksum
/// entry.  To make CHECKSUM entry size bounded, a Checksum entry can refer to a
/// previous Checksum entry so it does not have to repeat byte range that is
/// already covered by the previous entry.  The xxhash list contains one xxhash
/// per chunk. A chunk has (1 << chunk_size_logarithm) bytes.  The last chunk
/// can be incomplete.
struct MemChecksum {
    /// Indicates the "start" offset of the bytes that should be written
    /// on serialization.
    ///
    /// This is also the offset of the previous Checksum entry.
    start: u64,

    /// Indicates the "end" offset of the bytes that can be checked.
    ///
    /// This is also the offset of the current Checksum entry.
    end: u64,

    /// Tracks the chain length. Used to detect whether `checksum_max_chain_len`
    /// is exceeded or not.
    chain_len: u32,

    /// Each chunk has (1 << chunk_size_logarithmarithm) bytes. The last chunk
    /// can be shorter.
    chunk_size_logarithm: u32,

    /// Checksums per chunk.
    ///
    /// The start of a chunk always aligns with the chunk size, which might not
    /// match `start`. For example, given the `start` and `end` shown below:
    ///
    /// ```plain,ignore
    /// | chunk 1 (1MB) | chunk 2 (1MB) | chunk 3 (1MB) | chunk 4 (200K) |
    ///                     |<-start                                end->|
    /// ```
    ///
    /// The `xxhash_list` would be:
    ///
    /// ```plain,ignore
    /// [xxhash(chunk 2 (1MB)), xxhash(chunk 3 (1MB)), xxhash(chunk 4 (200K))]
    /// ```
    ///
    /// For the root checksum (`Index::chunksum`), the `xxhash_list` should
    /// convert the entire buffer, as if `start` is 0 (but `start` is not 0).
    xxhash_list: Vec<u64>,

    /// Whether chunks are checked against `xxhash_list`.
    ///
    /// Stored in a bit vector. `checked.len() * 64` should be >=
    /// `xxhash_list.len()`.
    checked: Vec<AtomicU64>,
}

/// Read reversed vlq at the given end offset (exclusive).
/// Return the decoded integer and the bytes used by the VLQ integer.
fn read_vlq_reverse(buf: &[u8], end_offset: usize) -> io::Result<(u64, usize)> {
    let mut int_buf = Vec::new();
    for i in (0..end_offset).rev() {
        int_buf.push(buf[i]);
        if buf[i] <= 127 {
            break;
        }
    }
    let (value, vlq_size) = int_buf.read_vlq_at(0)?;
    assert_eq!(vlq_size, int_buf.len());
    Ok((value, vlq_size))
}

// Offsets that are >= DIRTY_OFFSET refer to in-memory entries that haven't been
// written to disk. Offsets < DIRTY_OFFSET are on-disk offsets.
const DIRTY_OFFSET: u64 = 1u64 << 63;

const TYPE_HEAD: u8 = 0;
const TYPE_ROOT: u8 = 1;
const TYPE_RADIX: u8 = 2;
const TYPE_LEAF: u8 = 3;
const TYPE_LINK: u8 = 4;
const TYPE_KEY: u8 = 5;
const TYPE_EXT_KEY: u8 = 6;
const TYPE_INLINE_LEAF: u8 = 7;
const TYPE_CHECKSUM: u8 = 8;

// Bits needed to represent the above type integers.
const TYPE_BITS: usize = 3;

// Size constants. Do not change.
const TYPE_BYTES: usize = 1;
const RADIX_FLAG_BYTES: usize = 1;
const RADIX_BITMAP_BYTES: usize = 2;

// Bit flags used by radix
const RADIX_FLAG_USE_64BIT: u8 = 1;
const RADIX_FLAG_HAVE_LINK: u8 = 1 << 7;

/// Offset to an entry. The type of the entry is yet to be resolved.
#[derive(Copy, Clone, PartialEq, PartialOrd, Default)]
pub struct Offset(u64);

// Typed offsets. Constructed after verifying types.
// `LinkOffset` is public since it's exposed by some APIs.

#[derive(Copy, Clone, PartialEq, PartialOrd, Default)]
struct RadixOffset(Offset);
#[derive(Copy, Clone, PartialEq, PartialOrd, Default)]
struct LeafOffset(Offset);

/// Offset to a linked list entry.
///
/// The entry stores a [u64] integer and optionally, the next [`LinkOffset`].
#[derive(Copy, Clone, PartialEq, PartialOrd, Default)]
pub struct LinkOffset(Offset);
#[derive(Copy, Clone, PartialEq, PartialOrd, Default)]
struct KeyOffset(Offset);
#[derive(Copy, Clone, PartialEq, PartialOrd, Default)]
struct ExtKeyOffset(Offset);
#[derive(Copy, Clone, PartialEq, PartialOrd, Default)]
struct ChecksumOffset(Offset);

#[derive(Copy, Clone)]
enum TypedOffset {
    Radix(RadixOffset),
    Leaf(LeafOffset),
    Link(LinkOffset),
    Key(KeyOffset),
    ExtKey(ExtKeyOffset),
    Checksum(ChecksumOffset),
}

impl Offset {
    /// Convert an unverified `u64` read from disk to a non-dirty `Offset`.
    /// Return [`errors::IndexError`] if the offset is dirty.
    #[inline]
    fn from_disk(index: impl IndexBuf, value: u64) -> crate::Result<Self> {
        if value >= DIRTY_OFFSET {
            Err(index.corruption(format!("illegal disk offset {}", value)))
        } else {
            Ok(Offset(value))
        }
    }

    /// Convert a possibly "dirty" offset to a non-dirty offset.
    /// Useful when writing offsets to disk.
    #[inline]
    fn to_disk(self, offset_map: &OffsetMap) -> u64 {
        offset_map.get(self)
    }

    /// Convert to `TypedOffset`.
    fn to_typed(self, index: impl IndexBuf) -> crate::Result<TypedOffset> {
        let type_int = self.type_int(&index)?;
        match type_int {
            TYPE_RADIX => Ok(TypedOffset::Radix(RadixOffset(self))),
            TYPE_LEAF => Ok(TypedOffset::Leaf(LeafOffset(self))),
            TYPE_LINK => Ok(TypedOffset::Link(LinkOffset(self))),
            TYPE_KEY => Ok(TypedOffset::Key(KeyOffset(self))),
            TYPE_EXT_KEY => Ok(TypedOffset::ExtKey(ExtKeyOffset(self))),
            // LeafOffset handles inline transparently.
            TYPE_INLINE_LEAF => Ok(TypedOffset::Leaf(LeafOffset(self))),
            TYPE_CHECKSUM => Ok(TypedOffset::Checksum(ChecksumOffset(self))),
            _ => Err(index.corruption(format!("type {} is unsupported", type_int))),
        }
    }

    /// Read the `type_int` value.
    fn type_int(self, index: impl IndexBuf) -> crate::Result<u8> {
        let buf = index.buf();
        if self.is_null() {
            Err(index.corruption("invalid read from null"))
        } else if self.is_dirty() {
            Ok(((self.0 - DIRTY_OFFSET) & ((1 << TYPE_BITS) - 1)) as u8)
        } else {
            index.verify_checksum(self.0, TYPE_BYTES as u64)?;
            match buf.get(self.0 as usize) {
                Some(x) => Ok(*x),
                _ => Err(index.range_error(self.0 as usize, 1)),
            }
        }
    }

    /// Convert to `TypedOffset` without returning detailed error message.
    ///
    /// This is useful for cases where the error handling is optional.
    fn to_optional_typed(self, buf: &[u8]) -> Option<TypedOffset> {
        if self.is_null() {
            return None;
        }
        let type_int = if self.is_dirty() {
            Some(((self.0 - DIRTY_OFFSET) & ((1 << TYPE_BITS) - 1)) as u8)
        } else {
            buf.get(self.0 as usize).cloned()
        };
        match type_int {
            Some(TYPE_RADIX) => Some(TypedOffset::Radix(RadixOffset(self))),
            Some(TYPE_LEAF) => Some(TypedOffset::Leaf(LeafOffset(self))),
            Some(TYPE_LINK) => Some(TypedOffset::Link(LinkOffset(self))),
            Some(TYPE_KEY) => Some(TypedOffset::Key(KeyOffset(self))),
            Some(TYPE_EXT_KEY) => Some(TypedOffset::ExtKey(ExtKeyOffset(self))),
            // LeafOffset handles inline transparently.
            Some(TYPE_INLINE_LEAF) => Some(TypedOffset::Leaf(LeafOffset(self))),
            Some(TYPE_CHECKSUM) => Some(TypedOffset::Checksum(ChecksumOffset(self))),
            _ => None,
        }
    }

    /// Test whether the offset is null (0).
    #[inline]
    fn is_null(self) -> bool {
        self.0 == 0
    }

    /// Test whether the offset points to an in-memory entry.
    #[inline]
    fn is_dirty(self) -> bool {
        self.0 >= DIRTY_OFFSET
    }

    fn null() -> Self {
        Self(0)
    }
}

// Common methods shared by typed offset structs.
trait TypedOffsetMethods: Sized {
    #[inline]
    fn dirty_index(self) -> usize {
        debug_assert!(self.to_offset().is_dirty());
        ((self.to_offset().0 - DIRTY_OFFSET) >> TYPE_BITS) as usize
    }

    #[inline]
    fn from_offset(offset: Offset, index: impl IndexBuf) -> crate::Result<Self> {
        if offset.is_null() {
            Ok(Self::from_offset_unchecked(offset))
        } else {
            let type_int = offset.type_int(&index)?;
            if type_int == Self::type_int() {
                Ok(Self::from_offset_unchecked(offset))
            } else {
                Err(index.corruption(format!("inconsistent type at {:?}", offset)))
            }
        }
    }

    #[inline]
    fn from_dirty_index(index: usize) -> Self {
        Self::from_offset_unchecked(Offset(
            (((index as u64) << TYPE_BITS) | Self::type_int() as u64) + DIRTY_OFFSET,
        ))
    }

    fn type_int() -> u8;

    fn from_offset_unchecked(offset: Offset) -> Self;

    fn to_offset(&self) -> Offset;
}

impl_offset!(RadixOffset, TYPE_RADIX, "Radix");
impl_offset!(LeafOffset, TYPE_LEAF, "Leaf");
impl_offset!(LinkOffset, TYPE_LINK, "Link");
impl_offset!(KeyOffset, TYPE_KEY, "Key");
impl_offset!(ExtKeyOffset, TYPE_EXT_KEY, "ExtKey");
impl_offset!(ChecksumOffset, TYPE_CHECKSUM, "Checksum");

impl RadixOffset {
    /// Link offset of a radix entry.
    #[inline]
    fn link_offset(self, index: &Index) -> crate::Result<LinkOffset> {
        if self.is_dirty() {
            Ok(index.dirty_radixes[self.dirty_index()].link_offset)
        } else {
            let flag_start = TYPE_BYTES + usize::from(self);
            let flag = *index
                .buf
                .get(flag_start)
                .ok_or_else(|| index.range_error(flag_start, 1))?;
            index.verify_checksum(
                flag_start as u64,
                (RADIX_FLAG_BYTES + RADIX_BITMAP_BYTES) as u64,
            )?;

            if Self::parse_have_link_from_flag(flag) {
                let bitmap_start = flag_start + RADIX_FLAG_BYTES;
                let bitmap = Self::read_bitmap_unchecked(index, bitmap_start)?;
                let int_size = Self::parse_int_size_from_flag(flag);
                let link_offset =
                    bitmap_start + RADIX_BITMAP_BYTES + bitmap.count_ones() as usize * int_size;
                index.verify_checksum(link_offset as u64, int_size as u64)?;
                let raw_offset = Self::read_raw_int_unchecked(index, int_size, link_offset)?;
                Ok(LinkOffset::from_offset(
                    Offset::from_disk(index, raw_offset)?,
                    index,
                )?)
            } else {
                Ok(LinkOffset::default())
            }
        }
    }

    /// Lookup the `i`-th child inside a radix entry.
    /// Return stored offset, or `Offset(0)` if that child does not exist.
    #[inline]
    fn child(self, index: &Index, i: u8) -> crate::Result<Offset> {
        // "i" is not derived from user input.
        assert!(i < 16);
        if self.is_dirty() {
            Ok(index.dirty_radixes[self.dirty_index()].offsets[i as usize])
        } else {
            let flag_start = TYPE_BYTES + usize::from(self);
            let bitmap_start = flag_start + RADIX_FLAG_BYTES;
            // Integrity of "bitmap" is checked below to reduce calls to verify_checksum, since
            // this is a hot path.
            let bitmap = Self::read_bitmap_unchecked(index, bitmap_start)?;
            let has_child = (1u16 << i) & bitmap != 0;
            if has_child {
                let flag = *index
                    .buf
                    .get(flag_start)
                    .ok_or_else(|| index.range_error(flag_start, 1))?;
                let int_size = Self::parse_int_size_from_flag(flag);
                let skip_child_count = (((1u16 << i) - 1) & bitmap).count_ones() as usize;
                let child_offset = bitmap_start + RADIX_BITMAP_BYTES + skip_child_count * int_size;
                index.verify_checksum(
                    flag_start as u64,
                    (child_offset + int_size - flag_start) as u64,
                )?;
                let raw_offset = Self::read_raw_int_unchecked(index, int_size, child_offset)?;
                Ok(Offset::from_disk(index, raw_offset)?)
            } else {
                index.verify_checksum(bitmap_start as u64, RADIX_BITMAP_BYTES as u64)?;
                Ok(Offset::default())
            }
        }
    }

    /// Copy an on-disk entry to memory so it can be modified. Return new offset.
    /// If the offset is already in-memory, return it as-is.
    #[inline]
    fn copy(self, index: &mut Index) -> crate::Result<RadixOffset> {
        if self.is_dirty() {
            Ok(self)
        } else {
            let entry = MemRadix::read_from(index, u64::from(self))?;
            let len = index.dirty_radixes.len();
            index.dirty_radixes.push(entry);
            Ok(RadixOffset::from_dirty_index(len))
        }
    }

    /// Change a child of `MemRadix`. Panic if the offset points to an on-disk entry.
    #[inline]
    fn set_child(self, index: &mut Index, i: u8, value: Offset) {
        assert!(i < 16);
        if self.is_dirty() {
            index.dirty_radixes[self.dirty_index()].offsets[i as usize] = value;
        } else {
            panic!("bug: set_child called on immutable radix entry");
        }
    }

    /// Change link offset of `MemRadix`. Panic if the offset points to an on-disk entry.
    #[inline]
    fn set_link(self, index: &mut Index, value: LinkOffset) {
        if self.is_dirty() {
            index.dirty_radixes[self.dirty_index()].link_offset = value;
        } else {
            panic!("bug: set_link called on immutable radix entry");
        }
    }

    /// Change all children and link offset to null.
    /// Panic if the offset points to an on-disk entry.
    fn set_all_to_null(self, index: &mut Index) {
        if self.is_dirty() {
            index.dirty_radixes[self.dirty_index()] = MemRadix::default();
        } else {
            panic!("bug: set_all_to_null called on immutable radix entry");
        }
    }

    /// Create a new in-memory radix entry.
    #[inline]
    fn create(index: &mut Index, radix: MemRadix) -> RadixOffset {
        let len = index.dirty_radixes.len();
        index.dirty_radixes.push(radix);
        RadixOffset::from_dirty_index(len)
    }

    /// Parse whether link offset exists from a flag.
    #[inline]
    fn parse_have_link_from_flag(flag: u8) -> bool {
        flag & RADIX_FLAG_HAVE_LINK != 0
    }

    /// Parse int size (in bytes) from a flag.
    #[inline]
    fn parse_int_size_from_flag(flag: u8) -> usize {
        if flag & RADIX_FLAG_USE_64BIT == 0 {
            size_of::<u32>()
        } else {
            size_of::<u64>()
        }
    }

    /// Read bitmap from the given offset without integrity check.
    #[inline]
    fn read_bitmap_unchecked(index: &Index, bitmap_offset: usize) -> crate::Result<u16> {
        debug_assert_eq!(RADIX_BITMAP_BYTES, size_of::<u16>());
        let buf = &index.buf;
        buf.get(bitmap_offset..bitmap_offset + RADIX_BITMAP_BYTES)
            .map(LittleEndian::read_u16)
            .ok_or_else(|| {
                crate::Error::corruption(
                    &index.path,
                    format!("cannot read radix bitmap at {}", bitmap_offset),
                )
            })
    }

    /// Read integer from the given offset without integrity check.
    #[inline]
    fn read_raw_int_unchecked(index: &Index, int_size: usize, offset: usize) -> crate::Result<u64> {
        let buf = &index.buf;
        let result = match int_size {
            4 => buf
                .get(offset..offset + 4)
                .map(|buf| LittleEndian::read_u32(buf) as u64),
            8 => buf.get(offset..offset + 8).map(LittleEndian::read_u64),
            _ => unreachable!(),
        };
        result.ok_or_else(|| {
            crate::Error::corruption(
                &index.path,
                format!("cannot read {}-byte int at {}", int_size, offset),
            )
        })
    }
}

/// Extract key_content from an untyped Offset. Internal use only.
fn extract_key_content(index: &Index, key_offset: Offset) -> crate::Result<&[u8]> {
    let typed_offset = key_offset.to_typed(index)?;
    match typed_offset {
        TypedOffset::Key(x) => Ok(x.key_content(index)?),
        TypedOffset::ExtKey(x) => Ok(x.key_content(index)?),
        _ => Err(index.corruption(format!("unexpected key type at {}", key_offset.0))),
    }
}

impl LeafOffset {
    /// Key content and link offsets of a leaf entry.
    #[inline]
    fn key_and_link_offset(self, index: &Index) -> crate::Result<(&[u8], LinkOffset)> {
        if self.is_dirty() {
            let e = &index.dirty_leafs[self.dirty_index()];
            let key_content = extract_key_content(index, e.key_offset)?;
            Ok((key_content, e.link_offset))
        } else {
            let (key_content, raw_link_offset) = match index.buf[usize::from(self)] {
                TYPE_INLINE_LEAF => {
                    let raw_key_offset = u64::from(self) + TYPE_BYTES as u64;
                    // PERF: Consider skip checksum for this read.
                    let key_offset = ExtKeyOffset::from_offset(
                        Offset::from_disk(index, raw_key_offset)?,
                        index,
                    )?;
                    // Avoid using key_content. Skip one checksum check.
                    let (key_content, key_entry_size) =
                        key_offset.key_content_and_entry_size_unchecked(index)?;
                    let key_entry_size = key_entry_size.unwrap();
                    let raw_link_offset = raw_key_offset + key_entry_size as u64;
                    index.verify_checksum(u64::from(self), raw_link_offset - u64::from(self))?;
                    (key_content, raw_link_offset)
                }
                TYPE_LEAF => {
                    let (raw_key_offset, vlq_len): (u64, _) = index
                        .buf
                        .read_vlq_at(usize::from(self) + TYPE_BYTES)
                        .context(
                            index.path(),
                            "cannot read key_offset in LeafOffset::key_and_link_offset",
                        )
                        .corruption()?;
                    let key_offset = Offset::from_disk(index, raw_key_offset)?;
                    let key_content = extract_key_content(index, key_offset)?;
                    let (raw_link_offset, vlq_len2) = index
                        .buf
                        .read_vlq_at(usize::from(self) + TYPE_BYTES + vlq_len)
                        .context(
                            index.path(),
                            "cannot read link_offset in LeafOffset::key_and_link_offset",
                        )
                        .corruption()?;
                    index.verify_checksum(
                        u64::from(self),
                        (TYPE_BYTES + vlq_len + vlq_len2) as u64,
                    )?;
                    (key_content, raw_link_offset)
                }
                _ => unreachable!("bug: LeafOffset constructed with non-leaf types"),
            };
            let link_offset =
                LinkOffset::from_offset(Offset::from_disk(index, raw_link_offset)?, index)?;
            Ok((key_content, link_offset))
        }
    }

    /// Create a new in-memory leaf entry. The key entry cannot be null.
    #[inline]
    fn create(index: &mut Index, link_offset: LinkOffset, key_offset: Offset) -> LeafOffset {
        debug_assert!(!key_offset.is_null());
        let len = index.dirty_leafs.len();
        index.dirty_leafs.push(MemLeaf {
            link_offset,
            key_offset,
        });
        LeafOffset::from_dirty_index(len)
    }

    /// Update link_offset of a leaf entry in-place. Copy on write. Return the new leaf_offset
    /// if it's copied from disk.
    ///
    /// Note: the old leaf is expected to be no longer needed. If that's not true, don't call
    /// this function.
    #[inline]
    fn set_link(self, index: &mut Index, link_offset: LinkOffset) -> crate::Result<LeafOffset> {
        if self.is_dirty() {
            index.dirty_leafs[self.dirty_index()].link_offset = link_offset;
            Ok(self)
        } else {
            let entry = MemLeaf::read_from(index, u64::from(self))?;
            Ok(Self::create(index, link_offset, entry.key_offset))
        }
    }

    /// Mark the entry as unused. An unused entry won't be written to disk.
    /// No effect on an on-disk entry.
    fn mark_unused(self, index: &mut Index) {
        if self.is_dirty() {
            let key_offset = index.dirty_leafs[self.dirty_index()].key_offset;
            match key_offset.to_typed(&*index) {
                Ok(TypedOffset::Key(x)) => x.mark_unused(index),
                Ok(TypedOffset::ExtKey(x)) => x.mark_unused(index),
                _ => {}
            };
            index.dirty_leafs[self.dirty_index()].mark_unused()
        }
    }
}

/// Iterator for values in the linked list
pub struct LeafValueIter<'a> {
    index: &'a Index,
    offset: LinkOffset,
    errored: bool,
}

impl<'a> Iterator for LeafValueIter<'a> {
    type Item = crate::Result<u64>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset.is_null() || self.errored {
            None
        } else {
            match self.offset.value_and_next(self.index) {
                Ok((value, next)) => {
                    self.offset = next;
                    Some(Ok(value))
                }
                Err(e) => {
                    self.errored = true;
                    Some(Err(e))
                }
            }
        }
    }
}

impl<'a> LeafValueIter<'a> {
    pub fn is_empty(&self) -> bool {
        self.offset.is_null()
    }
}

/// Iterator returned by [`Index::range`].
/// Provide access to full keys and values (as [`LinkOffset`]), sorted by key.
pub struct RangeIter<'a> {
    index: &'a Index,

    // Stack about what is being visited, for `next`.
    front_stack: Vec<IterState>,

    // Stack about what is being visited, for `next_back`.
    back_stack: Vec<IterState>,

    // Completed. Either error out, or the iteration ends.
    completed: bool,
}

impl<'a> RangeIter<'a> {
    fn new(index: &'a Index, front_stack: Vec<IterState>, back_stack: Vec<IterState>) -> Self {
        assert!(!front_stack.is_empty());
        assert!(!back_stack.is_empty());
        Self {
            completed: front_stack.last() == back_stack.last(),
            index,
            front_stack,
            back_stack,
        }
    }

    /// Reconstruct "key" from the stack.
    fn key(stack: &[IterState], index: &Index) -> crate::Result<Vec<u8>> {
        // Reconstruct key. Collect base16 child stack (prefix + visiting),
        // then convert to base256.
        let mut prefix = Vec::with_capacity(stack.len() - 1);
        for frame in stack.iter().take(stack.len() - 1).cloned() {
            prefix.push(match frame {
                // The frame contains the "current" child being visited.
                IterState::RadixChild(_, child) if child < 16 => child,
                _ => unreachable!("bug: malicious iterator state"),
            })
        }
        if prefix.len() & 1 == 1 {
            // Odd-length key
            Err(index.corruption("unexpected odd-length key"))
        } else {
            Ok(base16_to_base256(&prefix))
        }
    }

    /// Used by both `next` and `next_back`.
    fn step(
        index: &'a Index,
        stack: &mut Vec<IterState>,
        towards: Side,
        exclusive: IterState,
    ) -> Option<crate::Result<(Cow<'a, [u8]>, LinkOffset)>> {
        loop {
            let state = match stack.pop().unwrap().step(towards) {
                // Pop. Visit next.
                None => continue,
                Some(state) => state,
            };

            if state == exclusive {
                // Stop iteration.
                return None;
            }

            // Write down what's being visited.
            stack.push(state);

            return match state {
                IterState::RadixChild(radix, child) => match radix.child(index, child) {
                    Ok(next_offset) if next_offset.is_null() => continue,
                    Ok(next_offset) => match next_offset.to_typed(index) {
                        Ok(TypedOffset::Radix(next_radix)) => {
                            stack.push(match towards {
                                Front => IterState::RadixEnd(next_radix),
                                Back => IterState::RadixStart(next_radix),
                            });
                            continue;
                        }
                        Ok(TypedOffset::Leaf(next_leaf)) => {
                            stack.push(match towards {
                                Front => IterState::LeafEnd(next_leaf),
                                Back => IterState::LeafStart(next_leaf),
                            });
                            continue;
                        }
                        Ok(_) => Some(Err(index.corruption("unexpected type during iteration"))),
                        Err(err) => Some(Err(err)),
                    },
                    Err(err) => Some(Err(err)),
                },
                IterState::RadixLeaf(radix) => match radix.link_offset(index) {
                    Ok(link_offset) if link_offset.is_null() => continue,
                    Ok(link_offset) => match Self::key(stack, index) {
                        Ok(key) => Some(Ok((Cow::Owned(key), link_offset))),
                        Err(err) => Some(Err(err)),
                    },
                    Err(err) => Some(Err(err)),
                },
                IterState::Leaf(leaf) => match leaf.key_and_link_offset(index) {
                    Ok((key, link_offset)) => Some(Ok((Cow::Borrowed(key), link_offset))),
                    Err(err) => Some(Err(err)),
                },
                IterState::RadixEnd(_)
                | IterState::RadixStart(_)
                | IterState::LeafStart(_)
                | IterState::LeafEnd(_) => {
                    continue;
                }
            };
        }
    }
}

impl<'a> Iterator for RangeIter<'a> {
    type Item = crate::Result<(Cow<'a, [u8]>, LinkOffset)>;

    /// Return the next key and corresponding [`LinkOffset`].
    fn next(&mut self) -> Option<Self::Item> {
        if self.completed {
            return None;
        }
        let exclusive = self.back_stack.last().cloned().unwrap();
        let result = Self::step(self.index, &mut self.front_stack, Back, exclusive);
        match result {
            Some(Err(_)) | None => self.completed = true,
            _ => {}
        }
        result
    }
}

impl<'a> DoubleEndedIterator for RangeIter<'a> {
    /// Return the next key and corresponding [`LinkOffset`], from the end of
    /// the iterator.
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.completed {
            return None;
        }
        let exclusive = self.front_stack.last().cloned().unwrap();
        let result = Self::step(self.index, &mut self.back_stack, Front, exclusive);
        match result {
            Some(Err(_)) | None => self.completed = true,
            _ => {}
        }
        result
    }
}

impl LinkOffset {
    /// Iterating through values referred by this linked list.
    pub fn values(self, index: &Index) -> LeafValueIter<'_> {
        LeafValueIter {
            errored: false,
            index,
            offset: self,
        }
    }

    /// Get value, and the next link offset.
    #[inline]
    fn value_and_next(self, index: &Index) -> crate::Result<(u64, LinkOffset)> {
        if self.is_dirty() {
            let e = &index.dirty_links[self.dirty_index()];
            Ok((e.value, e.next_link_offset))
        } else {
            let (value, vlq_len) = index
                .buf
                .read_vlq_at(usize::from(self) + TYPE_BYTES)
                .context(
                    index.path(),
                    "cannot read link_value in LinkOffset::value_and_next",
                )
                .corruption()?;
            let (next_link, vlq_len2) = index
                .buf
                .read_vlq_at(usize::from(self) + TYPE_BYTES + vlq_len)
                .context(
                    index.path(),
                    "cannot read next_link_offset in LinkOffset::value_and_next",
                )
                .corruption()?;
            index.verify_checksum(u64::from(self), (TYPE_BYTES + vlq_len + vlq_len2) as u64)?;
            let next_link = LinkOffset::from_offset(Offset::from_disk(index, next_link)?, index)?;
            Ok((value, next_link))
        }
    }

    /// Create a new link entry that chains this entry.
    /// Return new `LinkOffset`
    fn create(self, index: &mut Index, value: u64) -> LinkOffset {
        let new_link = MemLink {
            value,
            next_link_offset: self,
            unused: false,
        };
        let len = index.dirty_links.len();
        index.dirty_links.push(new_link);
        LinkOffset::from_dirty_index(len)
    }
}

impl KeyOffset {
    /// Key content of a key entry.
    #[inline]
    fn key_content(self, index: &Index) -> crate::Result<&[u8]> {
        if self.is_dirty() {
            Ok(&index.dirty_keys[self.dirty_index()].key[..])
        } else {
            let (key_len, vlq_len): (usize, _) = index
                .buf
                .read_vlq_at(usize::from(self) + TYPE_BYTES)
                .context(
                    index.path(),
                    "cannot read key_len in KeyOffset::key_content",
                )
                .corruption()?;
            let start = usize::from(self) + TYPE_BYTES + vlq_len;
            let end = start + key_len;
            index.verify_checksum(u64::from(self), end as u64 - u64::from(self))?;
            if end > index.buf.len() {
                Err(index.range_error(start, end - start))
            } else {
                Ok(&index.buf[start..end])
            }
        }
    }

    /// Create a new in-memory key entry. The key cannot be empty.
    #[inline]
    fn create(index: &mut Index, key: &[u8]) -> KeyOffset {
        debug_assert!(!key.is_empty());
        let len = index.dirty_keys.len();
        index.dirty_keys.push(MemKey {
            key: Vec::from(key).into_boxed_slice(),
        });
        KeyOffset::from_dirty_index(len)
    }

    /// Mark the entry as unused. An unused entry won't be written to disk.
    /// No effect on an on-disk entry.
    fn mark_unused(self, index: &mut Index) {
        if self.is_dirty() {
            index.dirty_keys[self.dirty_index()].mark_unused();
        }
    }
}

impl ExtKeyOffset {
    /// Key content of a key entry.
    #[inline]
    fn key_content(self, index: &Index) -> crate::Result<&[u8]> {
        let (key_content, entry_size) = self.key_content_and_entry_size_unchecked(index)?;
        if let Some(entry_size) = entry_size {
            index.verify_checksum(u64::from(self), entry_size as u64)?;
        }
        Ok(key_content)
    }

    /// Key content and key entry size. Used internally.
    #[inline]
    fn key_content_and_entry_size_unchecked(
        self,
        index: &Index,
    ) -> crate::Result<(&[u8], Option<usize>)> {
        let (start, len, entry_size) = if self.is_dirty() {
            let e = &index.dirty_ext_keys[self.dirty_index()];
            (e.start, e.len, None)
        } else {
            let (start, vlq_len1): (u64, _) = index
                .buf
                .read_vlq_at(usize::from(self) + TYPE_BYTES)
                .context(
                    index.path(),
                    "cannot read ext_key_start in ExtKeyOffset::key_content",
                )
                .corruption()?;
            let (len, vlq_len2): (u64, _) = index
                .buf
                .read_vlq_at(usize::from(self) + TYPE_BYTES + vlq_len1)
                .context(
                    index.path(),
                    "cannot read ext_key_len in ExtKeyOffset::key_content",
                )
                .corruption()?;
            (start, len, Some(TYPE_BYTES + vlq_len1 + vlq_len2))
        };
        let key_buf = index.key_buf.as_ref();
        let key_content = match key_buf.slice(start, len) {
            Some(k) => k,
            None => {
                return Err(index.corruption(format!(
                    "key buffer is invalid when reading referred keys at {}",
                    start
                )));
            }
        };
        Ok((key_content, entry_size))
    }

    /// Create a new in-memory external key entry. The key cannot be empty.
    #[inline]
    fn create(index: &mut Index, start: u64, len: u64) -> ExtKeyOffset {
        debug_assert!(len > 0);
        let i = index.dirty_ext_keys.len();
        index.dirty_ext_keys.push(MemExtKey { start, len });
        ExtKeyOffset::from_dirty_index(i)
    }

    /// Mark the entry as unused. An unused entry won't be written to disk.
    /// No effect on an on-disk entry.
    fn mark_unused(self, index: &mut Index) {
        if self.is_dirty() {
            index.dirty_ext_keys[self.dirty_index()].mark_unused();
        }
    }
}

/// Check type for an on-disk entry
fn check_type(index: impl IndexBuf, offset: usize, expected: u8) -> crate::Result<()> {
    let typeint = *(index
        .buf()
        .get(offset)
        .ok_or_else(|| index.range_error(offset, 1))?);
    if typeint != expected {
        Err(index.corruption(format!(
            "type mismatch at offset {} expected {} but got {}",
            offset, expected, typeint
        )))
    } else {
        Ok(())
    }
}

impl MemRadix {
    fn read_from(index: &Index, offset: u64) -> crate::Result<Self> {
        let buf = &index.buf;
        let offset = offset as usize;
        let mut pos = 0;

        // Integrity check is done at the end to reduce overhead.
        check_type(index, offset, TYPE_RADIX)?;
        pos += TYPE_BYTES;

        let flag = *buf
            .get(offset + pos)
            .ok_or_else(|| index.range_error(offset + pos, 1))?;
        pos += RADIX_FLAG_BYTES;

        let bitmap = RadixOffset::read_bitmap_unchecked(index, offset + pos)?;
        pos += RADIX_BITMAP_BYTES;

        let int_size = RadixOffset::parse_int_size_from_flag(flag);

        let mut offsets = [Offset::default(); 16];
        for (i, o) in offsets.iter_mut().enumerate() {
            if (bitmap >> i) & 1 == 1 {
                *o = Offset::from_disk(
                    index,
                    RadixOffset::read_raw_int_unchecked(index, int_size, offset + pos)?,
                )?;
                pos += int_size;
            }
        }

        let link_offset = if RadixOffset::parse_have_link_from_flag(flag) {
            let raw_offset = RadixOffset::read_raw_int_unchecked(index, int_size, offset + pos)?;
            pos += int_size;
            LinkOffset::from_offset(Offset::from_disk(index, raw_offset)?, index)?
        } else {
            LinkOffset::default()
        };

        index.verify_checksum(offset as u64, pos as u64)?;

        Ok(MemRadix {
            offsets,
            link_offset,
        })
    }

    fn write_to<W: Write>(&self, writer: &mut W, offset_map: &OffsetMap) -> io::Result<()> {
        // Prepare data to write
        let mut flag = 0;
        let mut bitmap = 0;
        let u32_max = u32::MAX as u64;

        let link_offset = if !self.link_offset.is_null() {
            flag |= RADIX_FLAG_HAVE_LINK;
            let link_offset = self.link_offset.to_disk(offset_map);
            if link_offset > u32_max {
                flag |= RADIX_FLAG_USE_64BIT;
            }
            link_offset
        } else {
            0
        };

        let mut child_offsets = [0u64; 16];
        for (i, child_offset) in self.offsets.iter().enumerate() {
            if !child_offset.is_null() {
                bitmap |= 1u16 << i;
                let child_offset = child_offset.to_disk(offset_map);
                if child_offset > u32_max {
                    flag |= RADIX_FLAG_USE_64BIT;
                }
                child_offsets[i] = child_offset;
            }
        }

        // Write them
        writer.write_all(&[TYPE_RADIX, flag])?;
        writer.write_u16::<LittleEndian>(bitmap)?;

        if flag & RADIX_FLAG_USE_64BIT != 0 {
            for &child_offset in child_offsets.iter() {
                if child_offset > 0 {
                    writer.write_u64::<LittleEndian>(child_offset)?;
                }
            }
            if link_offset > 0 {
                writer.write_u64::<LittleEndian>(link_offset)?;
            }
        } else {
            for &child_offset in child_offsets.iter() {
                if child_offset > 0 {
                    writer.write_u32::<LittleEndian>(child_offset as u32)?;
                }
            }
            if link_offset > 0 {
                writer.write_u32::<LittleEndian>(link_offset as u32)?;
            }
        }
        Ok(())
    }
}

impl MemLeaf {
    fn read_from(index: &Index, offset: u64) -> crate::Result<Self> {
        let buf = &index.buf;
        let offset = offset as usize;
        match buf.get(offset) {
            Some(&TYPE_INLINE_LEAF) => {
                let key_offset = offset + TYPE_BYTES;
                // Skip the key part
                let offset = key_offset + TYPE_BYTES;
                let (_key_start, vlq_len): (u64, _) = buf
                    .read_vlq_at(offset)
                    .context(index.path(), "cannot read key_start in MemLeaf::read_from")
                    .corruption()?;
                let offset = offset + vlq_len;
                let (_key_len, vlq_len): (u64, _) = buf
                    .read_vlq_at(offset)
                    .context(index.path(), "cannot read key_len in MemLeaf::read_from")
                    .corruption()?;
                let offset = offset + vlq_len;
                // Checksum will be verified by ExtKey and Leaf nodes
                let key_offset = Offset::from_disk(index, key_offset as u64)?;
                let link_offset =
                    LinkOffset::from_offset(Offset::from_disk(index, offset as u64)?, index)?;
                Ok(MemLeaf {
                    key_offset,
                    link_offset,
                })
            }
            Some(&TYPE_LEAF) => {
                let (key_offset, len1) = buf
                    .read_vlq_at(offset + TYPE_BYTES)
                    .context(index.path(), "cannot read key_offset in MemLeaf::read_from")
                    .corruption()?;
                let key_offset = Offset::from_disk(index, key_offset)?;
                let (link_offset, len2) = buf
                    .read_vlq_at(offset + TYPE_BYTES + len1)
                    .context(
                        index.path(),
                        "cannot read link_offset in MemLeaf::read_from",
                    )
                    .corruption()?;
                let link_offset =
                    LinkOffset::from_offset(Offset::from_disk(index, link_offset)?, index)?;
                index.verify_checksum(offset as u64, (TYPE_BYTES + len1 + len2) as u64)?;
                Ok(MemLeaf {
                    key_offset,
                    link_offset,
                })
            }
            _ => Err(index.range_error(offset, 1)),
        }
    }

    /// If the entry is suitable for writing inline, write an inline entry, mark dependent
    /// entries as "unused", and return `true`. Otherwise do nothing and return `false`.
    ///
    /// The caller probably wants to set this entry to "unused" to prevent writing twice,
    /// if true is returned.
    fn maybe_write_inline_to(
        &self,
        writer: &mut Vec<u8>,
        buf: &[u8],
        buf_offset: u64,
        dirty_ext_keys: &mut [MemExtKey],
        dirty_links: &mut [MemLink],
        offset_map: &mut OffsetMap,
    ) -> crate::Result<bool> {
        debug_assert!(!self.is_unused());

        // Conditions to be inlined:
        // - Both Key and Link are dirty (in-memory). Otherwise this might waste space.
        // - Key is ExtKey. This is just to make implementation easier. Owned key support might be
        // added in the future.
        // - Link does not refer to another in-memory link that hasn't been written yet (i.e.
        //   does not exist in offset_map). This is just to make implementation easier.

        let are_dependencies_dirty = self.key_offset.is_dirty() && self.link_offset.is_dirty();

        if are_dependencies_dirty {
            // Not being able to read the key offset is not a fatal error here - it
            // disables the inline optimization. But everything else works just fine.
            if let Some(TypedOffset::ExtKey(key_offset)) = self.key_offset.to_optional_typed(buf) {
                let ext_key_index = key_offset.dirty_index();
                let link_index = self.link_offset.dirty_index();
                let ext_key = dirty_ext_keys.get_mut(ext_key_index).unwrap();
                let link = dirty_links.get_mut(link_index).unwrap();

                let next_link_offset = link.next_link_offset;
                if next_link_offset.is_dirty()
                    && offset_map.link_map[next_link_offset.dirty_index()] == 0
                {
                    // Dependent Link is not written yet.
                    return Ok(false);
                }

                // Header
                writer.write_all(&[TYPE_INLINE_LEAF]).infallible()?;

                // Inlined ExtKey
                let offset = buf.len() as u64 + buf_offset;
                offset_map.ext_key_map[ext_key_index] = offset;
                ext_key.write_to(writer, offset_map).infallible()?;

                // Inlined Link
                let offset = buf.len() as u64 + buf_offset;
                offset_map.link_map[ext_key_index] = offset;
                link.write_to(writer, offset_map).infallible()?;

                ext_key.mark_unused();
                link.mark_unused();

                Ok(true)
            } else {
                // InlineLeaf only supports ExtKey, not embedded Key.
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    /// Write a Leaf entry.
    fn write_noninline_to<W: Write>(
        &self,
        writer: &mut W,
        offset_map: &OffsetMap,
    ) -> io::Result<()> {
        debug_assert!(!self.is_unused());
        writer.write_all(&[TYPE_LEAF])?;
        writer.write_vlq(self.key_offset.to_disk(offset_map))?;
        writer.write_vlq(self.link_offset.to_disk(offset_map))?;
        Ok(())
    }

    /// Mark the entry as unused. An unused entry won't be written to disk.
    fn mark_unused(&mut self) {
        self.key_offset = Offset::default();
    }

    #[inline]
    fn is_unused(&self) -> bool {
        self.key_offset.is_null()
    }
}

impl MemLink {
    fn read_from(index: impl IndexBuf, offset: u64) -> crate::Result<Self> {
        let buf = index.buf();
        let offset = offset as usize;
        check_type(&index, offset, TYPE_LINK)?;
        let (value, len1) = buf
            .read_vlq_at(offset + 1)
            .context(index.path(), "cannot read link_value in MemLink::read_from")
            .corruption()?;
        let (next_link_offset, len2) = buf
            .read_vlq_at(offset + TYPE_BYTES + len1)
            .context(
                index.path(),
                "cannot read next_link_offset in MemLink::read_from",
            )
            .corruption()?;
        let next_link_offset =
            LinkOffset::from_offset(Offset::from_disk(&index, next_link_offset)?, &index)?;
        index.verify_checksum(offset as u64, (TYPE_BYTES + len1 + len2) as u64)?;
        Ok(MemLink {
            value,
            next_link_offset,
            unused: false,
        })
    }

    fn write_to<W: Write>(&self, writer: &mut W, offset_map: &OffsetMap) -> io::Result<()> {
        writer.write_all(&[TYPE_LINK])?;
        writer.write_vlq(self.value)?;
        writer.write_vlq(self.next_link_offset.to_disk(offset_map))?;
        Ok(())
    }

    /// Mark the entry as unused. An unused entry won't be written to disk.
    fn mark_unused(&mut self) {
        self.unused = true;
    }

    #[inline]
    fn is_unused(&self) -> bool {
        self.unused
    }
}

impl MemKey {
    fn read_from(index: impl IndexBuf, offset: u64) -> crate::Result<Self> {
        let buf = index.buf();
        let offset = offset as usize;
        check_type(&index, offset, TYPE_KEY)?;
        let (key_len, len): (usize, _) = buf
            .read_vlq_at(offset + 1)
            .context(index.path(), "cannot read key_len in MemKey::read_from")
            .corruption()?;
        let key = Vec::from(
            buf.get(offset + TYPE_BYTES + len..offset + TYPE_BYTES + len + key_len)
                .ok_or_else(|| index.range_error(offset + TYPE_BYTES + len, key_len))?,
        )
        .into_boxed_slice();
        index.verify_checksum(offset as u64, (TYPE_BYTES + len + key_len) as u64)?;
        Ok(MemKey { key })
    }

    fn write_to<W: Write>(&self, writer: &mut W, _: &OffsetMap) -> io::Result<()> {
        writer.write_all(&[TYPE_KEY])?;
        writer.write_vlq(self.key.len())?;
        writer.write_all(&self.key)?;
        Ok(())
    }

    /// Mark the entry as unused. An unused entry won't be written to disk.
    fn mark_unused(&mut self) {
        self.key = Vec::new().into_boxed_slice();
    }

    #[inline]
    fn is_unused(&self) -> bool {
        self.key.len() == 0
    }
}

impl MemExtKey {
    fn read_from(index: impl IndexBuf, offset: u64) -> crate::Result<Self> {
        let buf = index.buf();
        let offset = offset as usize;
        check_type(&index, offset, TYPE_EXT_KEY)?;
        let (start, vlq_len1) = buf
            .read_vlq_at(offset + TYPE_BYTES)
            .context(
                index.path(),
                "cannot read ext_key_start in MemExtKey::read_from",
            )
            .corruption()?;
        let (len, vlq_len2) = buf
            .read_vlq_at(offset + TYPE_BYTES + vlq_len1)
            .context(
                index.path(),
                "cannot read ext_key_len in MemExtKey::read_from",
            )
            .corruption()?;
        index.verify_checksum(offset as u64, (TYPE_BYTES + vlq_len1 + vlq_len2) as u64)?;
        Ok(MemExtKey { start, len })
    }

    fn write_to<W: Write>(&self, writer: &mut W, _: &OffsetMap) -> io::Result<()> {
        writer.write_all(&[TYPE_EXT_KEY])?;
        writer.write_vlq(self.start)?;
        writer.write_vlq(self.len)?;
        Ok(())
    }

    /// Mark the entry as unused. An unused entry won't be written to disk.
    fn mark_unused(&mut self) {
        self.len = 0;
    }

    #[inline]
    fn is_unused(&self) -> bool {
        self.len == 0
    }
}

impl MemRoot {
    fn read_from(index: impl IndexBuf, offset: u64) -> crate::Result<(Self, usize)> {
        let offset = offset as usize;
        let mut cur = offset;
        check_type(&index, offset, TYPE_ROOT)?;
        cur += TYPE_BYTES;

        let (radix_offset, vlq_len) = index
            .buf()
            .read_vlq_at(cur)
            .context(index.path(), "cannot read radix_offset")
            .corruption()?;
        cur += vlq_len;

        let radix_offset =
            RadixOffset::from_offset(Offset::from_disk(&index, radix_offset)?, &index)?;

        let (meta_len, vlq_len): (usize, _) = index
            .buf()
            .read_vlq_at(cur)
            .context(index.path(), "cannot read meta_len")
            .corruption()?;
        cur += vlq_len;

        let meta = index
            .buf()
            .get(cur..cur + meta_len)
            .ok_or_else(|| index.range_error(cur, meta_len))?;
        cur += meta_len;

        index.verify_checksum(offset as u64, (cur - offset) as u64)?;
        Ok((
            MemRoot {
                radix_offset,
                meta: meta.to_vec().into_boxed_slice(),
            },
            cur - offset,
        ))
    }

    fn write_to<W: Write>(&self, writer: &mut W, offset_map: &OffsetMap) -> io::Result<usize> {
        let mut buf = Vec::with_capacity(16);
        buf.write_all(&[TYPE_ROOT])?;
        buf.write_vlq(self.radix_offset.to_disk(offset_map))?;
        buf.write_vlq(self.meta.len())?;
        buf.write_all(&self.meta)?;
        let len = buf.len();
        writer.write_all(&buf)?;
        Ok(len)
    }
}

impl MemChecksum {
    fn read_from(index: &impl IndexBuf, start_offset: u64) -> crate::Result<(Self, usize)> {
        let span = debug_span!("Checksum::read", offset = start_offset, chain_len = 0);
        let _entered = span.enter();
        let mut result = Self::default();
        let mut size: usize = 0;

        let mut offset = start_offset as usize;
        while offset > 0 {
            let mut cur: usize = offset;

            check_type(index, cur, TYPE_CHECKSUM)?;
            cur += TYPE_BYTES;

            let (previous_offset, vlq_len): (u64, _) = index
                .buf()
                .read_vlq_at(cur)
                .context(index.path(), "cannot read previous_checksum_offset")
                .corruption()?;
            cur += vlq_len;

            let (chunk_size_logarithm, vlq_len): (u32, _) = index
                .buf()
                .read_vlq_at(cur)
                .context(index.path(), "cannot read chunk_size_logarithm")
                .corruption()?;

            if chunk_size_logarithm > 31 {
                return Err(crate::Error::corruption(
                    index.path(),
                    format!(
                        "invalid chunk_size_logarithm {} at {}",
                        chunk_size_logarithm, cur
                    ),
                ));
            }
            cur += vlq_len;

            let chunk_size = 1usize << chunk_size_logarithm;
            let chunk_needed = (offset + chunk_size - 1) >> chunk_size_logarithm;

            let is_initial_checksum = start_offset == offset as u64;

            // Initialize our Self result in our first iteration.
            if is_initial_checksum {
                result.set_chunk_size_logarithm(index.buf(), chunk_size_logarithm)?;

                result.xxhash_list.resize(chunk_needed, 0);
                result.start = previous_offset;
                result.end = offset as u64;

                let checked_needed = (result.xxhash_list.len() + 63) / 64;
                result.checked.resize_with(checked_needed, Default::default);
            }
            result.chain_len = result.chain_len.saturating_add(1);

            //    0     1     2     3     4     5
            // |chunk|chunk|chunk|chunk|chunk|chunk|
            // |--------------|-----------------|---
            // 0              ^ previous        ^ offset
            //
            // The previous Checksum entry covers chunk 0,1,2(incomplete).
            // This Checksum entry covers chunk 2(complete),3,4,5(incomplete).

            // Read the new checksums.
            let start_chunk_index = (previous_offset >> chunk_size_logarithm) as usize;
            for i in start_chunk_index..chunk_needed {
                let incomplete_chunk = offset % chunk_size > 0 && i == chunk_needed - 1;

                // Don't record incomplete chunk hashes for previous checksums
                // since the "next" checksum (i.e. previous loop iteration) will
                // have already written the complete chunk's hash.
                if is_initial_checksum || !incomplete_chunk {
                    result.xxhash_list[i] = (&index.buf()[cur..])
                        .read_u64::<LittleEndian>()
                        .context(index.path(), "cannot read xxhash for checksum")?;
                }
                cur += 8;
            }

            // Check the checksum buffer itself.
            let xx32_read = (&index.buf()[cur..])
                .read_u32::<LittleEndian>()
                .context(index.path(), "cannot read xxhash32 for checksum")?;
            let xx32_self = xxhash32(&index.buf()[offset..cur]);
            if xx32_read != xx32_self {
                return Err(crate::Error::corruption(
                    index.path(),
                    format!(
                        "checksum at {} fails integrity check ({} != {})",
                        offset, xx32_read, xx32_self
                    ),
                ));
            }
            cur += 4;

            if is_initial_checksum {
                size = cur - offset;
            }

            offset = previous_offset as usize;
        }
        span.record("chain_len", result.chain_len);

        Ok((result, size))
    }

    /// Incrementally update the checksums so it covers the file content + `append_buf`.
    /// Assume the file content is append-only.
    /// Call this before calling `write_to`.
    fn update(
        &mut self,
        old_buf: &[u8],
        file: &mut File,
        file_len: u64,
        append_buf: &[u8],
    ) -> io::Result<()> {
        let start_chunk_index = (self.end >> self.chunk_size_logarithm) as usize;
        let start_chunk_offset = (start_chunk_index as u64) << self.chunk_size_logarithm;

        // Check the range that is being rewritten.
        let old_end = (old_buf.len() as u64).min(self.end);
        if old_end > start_chunk_offset {
            self.check_range(old_buf, start_chunk_offset, old_end - start_chunk_offset)?;
        }

        let new_total_len = file_len + append_buf.len() as u64;
        let chunk_size = 1u64 << self.chunk_size_logarithm;
        let chunk_needed = ((new_total_len + chunk_size - 1) >> self.chunk_size_logarithm) as usize;
        self.xxhash_list.resize(chunk_needed, 0);

        if self.end > file_len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "unexpected truncation",
            ));
        }

        //        start_chunk_offset (start of chunk including self.end)
        //                   v
        //                   |(1)|
        // |chunk|chunk|chunk|chunk|chunk|chunk|chunk|chunk|chunk|
        //                   |---file_buf---|
        // |--------------file--------------|
        // |-------old_buf-------|
        // |-----(2)-----|--(3)--|---(4)----|---append_buf---|
        //               ^       ^          ^                ^
        //          self.start self.end   file_len     new_total_len
        //
        // (1): range being re-checksummed
        // (2): covered by the previous Checksum
        // (3): covered by the current Checksum
        // (4): range written on-disk after `Index::open` (ex. by another process)
        //
        // old_buf:    buffer read at open time.
        // file:       buffer read at flush time (right now, with a lock).
        // append_buf: buffer to append to the file (protected by the same lock).

        let file_buf = {
            let mut file_buf = vec![0; (file_len - start_chunk_offset) as usize];
            file.seek(SeekFrom::Start(start_chunk_offset))?;
            file.read_exact(&mut file_buf)?;
            file_buf
        };

        for i in start_chunk_index..self.xxhash_list.len() {
            let start = (i as u64) << self.chunk_size_logarithm;
            let end = (start + chunk_size).min(new_total_len);
            let mut xx = XxHash::default();
            // Hash portions of file_buf that intersect with start..end.
            let file_buf_start = ((start - start_chunk_offset) as usize).min(file_buf.len());
            let file_buf_end = ((end - start_chunk_offset) as usize).min(file_buf.len());
            xx.write(&file_buf[file_buf_start..file_buf_end]);
            // Hash portions of append_buf that intersect with start..end.
            let append_buf_start = (start.max(file_len) - file_len) as usize;
            let append_buf_end = (end.max(file_len) - file_len) as usize;
            xx.write(&append_buf[append_buf_start..append_buf_end]);
            self.xxhash_list[i] = xx.finish();
        }

        // Update start and end:
        //
        //      chunk | chunk
        //      start   end (both are valid ChecksumOffset, in different chunks)
        //      v       v
        // old: |-------|
        // new:         |--------|----(1)----|
        //              ^        ^
        //              start    new_total_len
        //
        // (1): Place that this Checksum will be written to.
        //
        // Avoid updating start if start and end are in a same chunk:
        //
        //       -----chunk----|
        //       start     end
        //       v         v
        // old:  |---------|
        // new:  |--------------------|
        //       ^                    ^
        //       start                end
        //
        // This makes the length of chain of Checksums O(len(chunks)).
        if (self.start >> self.chunk_size_logarithm) != (self.end >> self.chunk_size_logarithm) {
            self.start = self.end;
        }
        self.end = new_total_len;

        Ok(())
    }

    /// Extend the checksum range to cover the entire range of the index buffer
    /// so the next open would only read O(1) checksum entries.
    fn flatten(&mut self) {
        self.start = 0;
        self.chain_len = 1;
    }

    fn write_to<W: Write>(&self, writer: &mut W, _offset_map: &OffsetMap) -> io::Result<usize> {
        let mut buf = Vec::with_capacity(16);
        buf.write_all(&[TYPE_CHECKSUM])?;
        buf.write_vlq(self.start)?;
        // self.end is implied by the write position.
        buf.write_vlq(self.chunk_size_logarithm)?;
        for &xx in self.xxhash_list_to_write() {
            buf.write_u64::<LittleEndian>(xx)?;
        }
        let xx32 = xxhash32(&buf);
        buf.write_u32::<LittleEndian>(xx32)?;
        writer.write_all(&buf)?;
        Ok(buf.len())
    }

    /// Return a sub list of the xxhash list that is not covered by the
    /// previous Checksum entry. It's used for serialization.
    fn xxhash_list_to_write(&self) -> &[u64] {
        // See the comment in `read_from` for `start_chunk_index`.
        // It is the starting index for
        let start_chunk_index = (self.start >> self.chunk_size_logarithm) as usize;
        &self.xxhash_list[start_chunk_index..]
    }

    /// Reset the `chunk_size_logarithm`.
    fn set_chunk_size_logarithm(
        &mut self,
        _buf: &[u8],
        chunk_size_logarithm: u32,
    ) -> crate::Result<()> {
        // Change chunk_size_logarithm if the checksum list is empty.
        if self.xxhash_list.is_empty() {
            self.chunk_size_logarithm = chunk_size_logarithm;
        }
        // NOTE: Consider re-hashing and allow changing the chunk_size_logarithm.
        Ok(())
    }

    /// Check a range of bytes.
    ///
    /// Depending on `chunk_size_logarithm`, bytes outside the specified range
    /// might also be checked.
    #[inline]
    fn check_range(&self, buf: &[u8], offset: u64, length: u64) -> io::Result<()> {
        if length == 0 || !self.is_enabled() {
            return Ok(());
        }

        // Ranges not covered by checksums are treated as bad.
        if offset + length > self.end {
            return checksum_error(self, offset, length);
        }

        // Otherwise, scan related chunks.
        let start = (offset >> self.chunk_size_logarithm) as usize;
        let end = ((offset + length - 1) >> self.chunk_size_logarithm) as usize;
        if !(start..=end).all(|i| self.check_chunk(buf, i)) {
            return checksum_error(self, offset, length);
        }
        Ok(())
    }

    /// Check the i-th chunk. The callsite must make sure `index` is within range.
    #[inline]
    fn check_chunk(&self, buf: &[u8], index: usize) -> bool {
        debug_assert!(index < self.xxhash_list.len());
        let bit = 1 << (index % 64);
        let checked = &self.checked[index / 64];
        if (checked.load(Acquire) & bit) == bit {
            true
        } else {
            let start = index << self.chunk_size_logarithm;
            let end = (self.end as usize).min((index + 1) << self.chunk_size_logarithm);
            if start == end {
                return true;
            }
            let hash = xxhash(&buf[start..end]);
            if hash == self.xxhash_list[index] {
                checked.fetch_or(bit, AcqRel);
                true
            } else {
                false
            }
        }
    }

    #[inline]
    fn is_enabled(&self) -> bool {
        self.end > 0
    }
}

fn write_reversed_vlq(mut writer: impl Write, value: usize) -> io::Result<()> {
    let mut reversed_vlq = Vec::new();
    reversed_vlq.write_vlq(value)?;
    reversed_vlq.reverse();
    writer.write_all(&reversed_vlq)?;
    Ok(())
}

impl Clone for MemChecksum {
    fn clone(&self) -> Self {
        Self {
            start: self.start,
            end: self.end,
            chain_len: self.chain_len,
            chunk_size_logarithm: self.chunk_size_logarithm,
            xxhash_list: self.xxhash_list.clone(),
            checked: self
                .checked
                .iter()
                .map(|c| c.load(Relaxed))
                .map(AtomicU64::new)
                .collect(),
        }
    }
}

impl Default for MemChecksum {
    fn default() -> Self {
        Self {
            start: 0,
            end: 0,
            chain_len: 0,
            chunk_size_logarithm: 20, // chunk_size: 1MB.
            xxhash_list: Vec::new(),
            checked: Vec::new(),
        }
    }
}

#[derive(Default)]
struct OffsetMap {
    radix_len: usize,
    radix_map: Vec<u64>,
    leaf_map: Vec<u64>,
    link_map: Vec<u64>,
    key_map: Vec<u64>,
    ext_key_map: Vec<u64>,
}

/// A simple structure that implements the IndexBuf interface.
/// It does not check checksum.
struct SimpleIndexBuf<'a>(&'a [u8], &'a Path);

impl<'a> IndexBuf for SimpleIndexBuf<'a> {
    fn buf(&self) -> &[u8] {
        self.0
    }
    fn verify_checksum(&self, _start: u64, _length: u64) -> crate::Result<()> {
        Ok(())
    }
    fn path(&self) -> &Path {
        self.1
    }
}

impl OffsetMap {
    fn empty_for_index(index: &Index) -> OffsetMap {
        let radix_len = index.dirty_radixes.len();
        OffsetMap {
            radix_len,
            radix_map: vec![0; radix_len],
            leaf_map: vec![0; index.dirty_leafs.len()],
            link_map: vec![0; index.dirty_links.len()],
            key_map: vec![0; index.dirty_keys.len()],
            ext_key_map: vec![0; index.dirty_ext_keys.len()],
        }
    }

    #[inline]
    fn get(&self, offset: Offset) -> u64 {
        if offset.is_dirty() {
            let dummy = SimpleIndexBuf(b"", Path::new("<dummy>"));
            let result = match offset.to_typed(dummy).unwrap() {
                // Radix entries are pushed in the reversed order. So the index needs to be
                // reversed.
                TypedOffset::Radix(x) => self.radix_map[self.radix_len - 1 - x.dirty_index()],
                TypedOffset::Leaf(x) => self.leaf_map[x.dirty_index()],
                TypedOffset::Link(x) => self.link_map[x.dirty_index()],
                TypedOffset::Key(x) => self.key_map[x.dirty_index()],
                TypedOffset::ExtKey(x) => self.ext_key_map[x.dirty_index()],
                TypedOffset::Checksum(_) => {
                    panic!("bug: ChecksumOffset shouldn't be used in OffsetMap::get")
                }
            };
            // result == 0 means an entry marked "unused" is actually used. It's a logic error.
            debug_assert!(result > 0);
            result
        } else {
            // No need to translate.
            offset.0
        }
    }
}

/// Choose between Front and Back. Used by [`RangeIter`] related logic.
#[derive(Clone, Copy)]
enum Side {
    Front,
    Back,
}
use Side::Back;
use Side::Front;

/// State used by [`RangeIter`].
#[derive(Clone, Copy, PartialEq, Debug)]
enum IterState {
    /// Visiting the child of a radix node.
    /// child must be inside 0..16 range.
    RadixChild(RadixOffset, u8),

    /// Visiting the leaf of a radix node.
    RadixLeaf(RadixOffset),

    /// Visiting this leaf node.
    Leaf(LeafOffset),

    /// Dummy states to express "inclusive" bounds.
    /// RadixStart < RadixLeaf < RadixChild < RadixEnd.
    /// LeafStart < Leaf < LeafEnd.
    RadixStart(RadixOffset),
    RadixEnd(RadixOffset),
    LeafStart(LeafOffset),
    LeafEnd(LeafOffset),
}

impl IterState {
    /// Get the next state on the same frame.
    /// Return `None` if the frame should be popped.
    fn next(self) -> Option<Self> {
        match self {
            IterState::RadixChild(radix, 15) => Some(IterState::RadixEnd(radix)),
            IterState::RadixChild(radix, i) => Some(IterState::RadixChild(radix, i + 1)),
            IterState::RadixStart(radix) => Some(IterState::RadixLeaf(radix)),
            IterState::RadixLeaf(radix) => Some(IterState::RadixChild(radix, 0)),
            IterState::LeafStart(leaf) => Some(IterState::Leaf(leaf)),
            IterState::Leaf(leaf) => Some(IterState::LeafEnd(leaf)),
            _ => None,
        }
    }

    /// Get the previous state on the same frame.
    /// Return `None` if the frame should be popped.
    fn prev(self) -> Option<Self> {
        match self {
            IterState::RadixChild(radix, 0) => Some(IterState::RadixLeaf(radix)),
            IterState::RadixChild(radix, i) => Some(IterState::RadixChild(radix, i - 1)),
            IterState::RadixEnd(radix) => Some(IterState::RadixChild(radix, 15)),
            IterState::RadixLeaf(radix) => Some(IterState::RadixStart(radix)),
            IterState::LeafEnd(leaf) => Some(IterState::Leaf(leaf)),
            IterState::Leaf(leaf) => Some(IterState::LeafStart(leaf)),
            _ => None,
        }
    }

    /// Move one step towards the given side.
    fn step(self, towards: Side) -> Option<Self> {
        match towards {
            Front => self.prev(),
            Back => self.next(),
        }
    }
}

/// Main Index
///
/// Insertion-only mapping from `bytes` to a list of [u64]s.
///
/// An [`Index`] is backed by an append-only file in the filesystem. Internally,
/// it uses base16 radix trees for keys and linked list for [u64] values. The
/// file format was designed to be able to support other types of indexes (ex.
/// non-radix-trees). Though none of them are implemented.
pub struct Index {
    // For locking and low-level access.
    file: Option<File>,

    // For efficient and shared random reading.
    // Backed by mmap.
    pub(crate) buf: Bytes,

    // For error messages.
    // Log uses this field for error messages.
    pub(crate) path: PathBuf,

    // Options
    checksum_enabled: bool,
    checksum_max_chain_len: u32,
    fsync: bool,
    write: Option<bool>,

    // Used by `clear_dirty`.
    clean_root: MemRoot,

    // In-memory entries. The root entry is always in-memory.
    dirty_root: MemRoot,
    dirty_radixes: Vec<MemRadix>,
    dirty_leafs: Vec<MemLeaf>,
    dirty_links: Vec<MemLink>,
    dirty_keys: Vec<MemKey>,
    dirty_ext_keys: Vec<MemExtKey>,

    checksum: MemChecksum,

    // Additional buffer for external keys.
    // Log::sync needs write access to this field.
    pub(crate) key_buf: Arc<dyn ReadonlyBuffer + Send + Sync>,

    // Emulate errors during flush.
    #[cfg(test)]
    fail_on_flush: u16,
}

/// Abstraction of the "external key buffer".
///
/// This makes it possible to use non-contiguous memory for a buffer,
/// and expose them as if it's contiguous.
pub trait ReadonlyBuffer {
    /// Get a slice using the given offset.
    fn slice(&self, start: u64, len: u64) -> Option<&[u8]>;
}

impl<T: AsRef<[u8]>> ReadonlyBuffer for T {
    #[inline]
    fn slice(&self, start: u64, len: u64) -> Option<&[u8]> {
        self.as_ref().get(start as usize..(start + len) as usize)
    }
}

/// Key to insert. Used by [Index::insert_advanced].
pub enum InsertKey<'a> {
    /// Embedded key.
    Embed(&'a [u8]),

    /// Reference (`[start, end)`) to `key_buf`.
    Reference((u64, u64)),
}

/// Options used to configured how an [`Index`] is opened.
///
/// Similar to [std::fs::OpenOptions], to use this, first call `new`, then
/// chain calls to methods to set each option, finally call `open` to get
/// an [`Index`] structure.
#[derive(Clone)]
pub struct OpenOptions {
    checksum_max_chain_len: u32,
    checksum_chunk_size_logarithm: u32,
    checksum_enabled: bool,
    fsync: bool,
    len: Option<u64>,
    write: Option<bool>,
    key_buf: Option<Arc<dyn ReadonlyBuffer + Send + Sync>>,
}

impl OpenOptions {
    #[allow(clippy::new_without_default)]
    /// Create [`OpenOptions`] with default configuration:
    /// - checksum enabled, with 1MB chunk size
    /// - checksum max chain length is `config::INDEX_CHECKSUM_MAX_CHAIN_LEN` (default: 10)
    /// - no external key buffer
    /// - no fsync
    /// - read root entry from the end of the file
    /// - open as read-write but fallback to read-only
    pub fn new() -> OpenOptions {
        OpenOptions {
            checksum_max_chain_len: config::INDEX_CHECKSUM_MAX_CHAIN_LEN.load(Acquire),
            checksum_chunk_size_logarithm: 20,
            checksum_enabled: true,
            fsync: false,
            len: None,
            write: None,
            key_buf: None,
        }
    }

    /// Set the maximum checksum chain length.
    ///
    /// If it is non-zero, and the checksum chain (linked list of checksum
    /// entries needed to verify the entire index) exceeds the specified length,
    /// they will be collapsed into a single checksum entry to make `open` more
    /// efficient.
    pub fn checksum_max_chain_len(&mut self, len: u32) -> &mut Self {
        self.checksum_max_chain_len = len;
        self
    }

    /// Set checksum chunk size as `1 << checksum_chunk_size_logarithm`.
    pub fn checksum_chunk_size_logarithm(
        &mut self,
        checksum_chunk_size_logarithm: u32,
    ) -> &mut Self {
        self.checksum_chunk_size_logarithm = checksum_chunk_size_logarithm;
        self
    }

    /// Set whether to write checksum entries on `flush`.
    pub fn checksum_enabled(&mut self, checksum_enabled: bool) -> &mut Self {
        self.checksum_enabled = checksum_enabled;
        self
    }

    /// Set fsync behavior.
    ///
    /// If true, then [`Index::flush`] will use `fsync` to flush data to the
    /// physical device before returning.
    pub fn fsync(&mut self, fsync: bool) -> &mut Self {
        self.fsync = fsync;
        self
    }

    /// Set whether writing is required:
    ///
    /// - `None`: open as read-write but fallback to read-only. `flush()` may fail.
    /// - `Some(false)`: open as read-only. `flush()` will always fail.
    /// - `Some(true)`: open as read-write. `open()` fails if read-write is not
    ///   possible. `flush()` will not fail due to permission issues.
    ///
    /// Note:  The index is always mutable in-memory. Only `flush()` may fail.
    pub fn write(&mut self, value: Option<bool>) -> &mut Self {
        self.write = value;
        self
    }

    /// Specify the logical length of the file.
    ///
    /// If `len` is `None`, use the actual file length. Otherwise, use the
    /// length specified by `len`. Reading the file length requires locking.
    ///
    /// This is useful for lock-free reads, or accessing to multiple versions of
    /// the index at the same time.
    ///
    /// To get a valid logical length, check the return value of [`Index::flush`].
    pub fn logical_len(&mut self, len: Option<u64>) -> &mut Self {
        self.len = len;
        self
    }

    /// Specify the external key buffer.
    ///
    /// With an external key buffer, keys could be stored as references using
    /// `index.insert_advanced` to save space.
    pub fn key_buf(&mut self, buf: Option<Arc<dyn ReadonlyBuffer + Send + Sync>>) -> &mut Self {
        self.key_buf = buf;
        self
    }

    /// Open the index file with given options.
    ///
    /// Driven by the "immutable by default" idea, together with append-only
    /// properties, [`OpenOptions::open`] returns a "snapshotted" view of the
    /// index. Changes to the filesystem won't change instantiated [`Index`]es.
    pub fn open<P: AsRef<Path>>(&self, path: P) -> crate::Result<Index> {
        let path = path.as_ref();
        let result: crate::Result<_> = (|| {
            let span = debug_span!("Index::open", path = path.to_string_lossy().as_ref());
            let _guard = span.enter();

            let mut open_options = self.clone();
            let open_result = if self.write == Some(false) {
                fs::OpenOptions::new().read(true).open(path)
            } else {
                fs::OpenOptions::new()
                    .read(true)
                    .create(true)
                    .append(true)
                    .open(path)
            };
            let mut file = match self.write {
                Some(write) => open_result.context(
                    path,
                    if write {
                        "cannot open Index with read-write mode"
                    } else {
                        "cannot open Index with read-only mode"
                    },
                )?,
                None => {
                    // Fall back to open the file as read-only, automatically.
                    match open_result {
                        Err(_) => {
                            open_options.write = Some(false);
                            fs::OpenOptions::new()
                                .read(true)
                                .open(path)
                                .context(path, "cannot open Index with read-only mode")?
                        }
                        Ok(file) => file,
                    }
                }
            };

            let bytes = {
                match self.len {
                    None => {
                        // Take the lock to read file length, since that decides root entry location.
                        let lock = ScopedFileLock::new(&mut file, false)
                            .context(path, "cannot lock Log to read file length")?;
                        mmap_bytes(lock.as_ref(), None).context(path, "cannot mmap")?
                    }
                    Some(len) => {
                        // No need to lock for getting file length.
                        mmap_bytes(&file, Some(len)).context(path, "cannot mmap")?
                    }
                }
            };

            let (dirty_radixes, clean_root, mut checksum) = if bytes.is_empty() {
                // Empty file. Create root radix entry as an dirty entry, and
                // rebuild checksum table (in case it's corrupted).
                let radix_offset = RadixOffset::from_dirty_index(0);
                let _ = utils::fix_perm_file(&file, false);
                let meta = Default::default();
                let root = MemRoot { radix_offset, meta };
                let checksum = MemChecksum::default();
                (vec![MemRadix::default()], root, checksum)
            } else {
                let end = bytes.len();
                let (root, mut checksum) = read_root_checksum_at_end(path, &bytes, end)?;
                if !self.checksum_enabled {
                    checksum = MemChecksum::default();
                }
                (vec![], root, checksum)
            };

            checksum.set_chunk_size_logarithm(&bytes, self.checksum_chunk_size_logarithm)?;
            let key_buf = self.key_buf.clone();
            let dirty_root = clean_root.clone();

            let index = Index {
                file: Some(file),
                buf: bytes,
                path: path.to_path_buf(),
                // Deconstruct open_options instead of storing it whole, since it contains a
                // permanent reference to the original key_buf mmap.
                checksum_enabled: open_options.checksum_enabled,
                checksum_max_chain_len: open_options.checksum_max_chain_len,
                fsync: open_options.fsync,
                write: open_options.write,
                clean_root,
                dirty_root,
                checksum,
                dirty_radixes,
                dirty_links: vec![],
                dirty_leafs: vec![],
                dirty_keys: vec![],
                dirty_ext_keys: vec![],
                key_buf: key_buf.unwrap_or_else(|| Arc::new(&b""[..])),
                #[cfg(test)]
                fail_on_flush: 0,
            };

            Ok(index)
        })();
        result.context(|| format!("in index::OpenOptions::open({:?})", path))
    }

    /// Create an in-memory [`Index`] that skips flushing to disk.
    /// Return an error if `checksum_chunk_size_logarithm` is not 0.
    pub fn create_in_memory(&self) -> crate::Result<Index> {
        let result: crate::Result<_> = (|| {
            let buf = Bytes::new();
            let dirty_radixes = vec![MemRadix::default()];
            let clean_root = {
                let radix_offset = RadixOffset::from_dirty_index(0);
                let meta = Default::default();
                MemRoot { radix_offset, meta }
            };
            let key_buf = self.key_buf.clone();
            let dirty_root = clean_root.clone();
            let mut checksum = MemChecksum::default();
            checksum.set_chunk_size_logarithm(&buf, self.checksum_chunk_size_logarithm)?;

            Ok(Index {
                file: None,
                buf,
                path: PathBuf::new(),
                checksum_enabled: self.checksum_enabled,
                checksum_max_chain_len: self.checksum_max_chain_len,
                fsync: self.fsync,
                write: self.write,
                clean_root,
                dirty_root,
                checksum,
                dirty_radixes,
                dirty_links: vec![],
                dirty_leafs: vec![],
                dirty_keys: vec![],
                dirty_ext_keys: vec![],
                key_buf: key_buf.unwrap_or_else(|| Arc::new(&b""[..])),
                #[cfg(test)]
                fail_on_flush: 0,
            })
        })();
        result.context("in index::OpenOptions::create_in_memory")
    }
}

/// Load root and checksum from the logical end.
fn read_root_checksum_at_end(
    path: &Path,
    bytes: &[u8],
    end: usize,
) -> crate::Result<(MemRoot, MemChecksum)> {
    // root_offset      (root_offset + root_size)
    // v                v
    // |---root_entry---|---checksum_entry---|--reversed_vlq_size---|
    // |<--------- root_checksum_size ------>|
    // |<-- root_size ->|

    let buf = SimpleIndexBuf(bytes, path);
    // Be careful! SimpleIndexBuf does not do data verification.
    // Handle integer range overflows here.
    let (root_checksum_size, vlq_size) =
        read_vlq_reverse(bytes, end).context(path, "cannot read len(root+checksum)")?;

    // Verify the header byte.
    check_type(&buf, 0, TYPE_HEAD)?;

    if end < root_checksum_size as usize + vlq_size {
        return Err(crate::Error::corruption(
            path,
            format!(
                "data corrupted at {} (invalid size: {})",
                end, root_checksum_size
            ),
        ));
    }

    let root_offset = end - root_checksum_size as usize - vlq_size;
    let (root, root_size) = MemRoot::read_from(&buf, root_offset as u64)?;

    let checksum = if root_offset + root_size + vlq_size == end {
        // No checksum - checksum disabled.
        MemChecksum::default()
    } else {
        let (checksum, _checksum_len) =
            MemChecksum::read_from(&buf, (root_offset + root_size) as u64)?;

        // Not checking lengths here:
        //
        // root_offset + root_size + checksum_len + vlq_size == end
        // so we can add new kinds of data in the future without breaking
        // older clients, similar to how Checksum gets added while
        // maintaining compatibility.

        // Verify the Root entry, since SimpleIndexBuf skips checksum check.
        // Checksum is self-verified. So no need to check it.
        checksum
            .check_range(bytes, root_offset as u64, root_size as u64)
            .context(path, "failed to verify Root entry")?;
        checksum
    };

    Ok((root, checksum))
}

impl fmt::Debug for OpenOptions {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "OpenOptions {{ ")?;
        write!(
            f,
            "checksum_chunk_size_logarithm: {}, ",
            self.checksum_chunk_size_logarithm
        )?;
        write!(f, "fsync: {}, ", self.fsync)?;
        write!(f, "len: {:?}, ", self.len)?;
        write!(f, "write: {:?}, ", self.write)?;
        let key_buf_desc = match self.key_buf {
            Some(ref _buf) => "Some(_)",
            None => "None",
        };
        write!(f, "key_buf: {} }}", key_buf_desc)?;
        Ok(())
    }
}

/// A subset of Index features for read-only accesses.
/// - Provides the main buffer, immutable data serialized on-disk.
/// - Provides the optional checksum checker.
/// - Provides the path (for error message).
trait IndexBuf {
    fn buf(&self) -> &[u8];
    fn path(&self) -> &Path;

    /// Verify checksum for the given range. Internal API used by `*Offset` structs.
    fn verify_checksum(&self, start: u64, length: u64) -> crate::Result<()>;

    #[inline(never)]
    fn range_error(&self, start: usize, length: usize) -> crate::Error {
        self.corruption(format!(
            "byte range {}..{} is unavailable",
            start,
            start + length
        ))
    }

    #[inline(never)]
    fn corruption(&self, message: impl ToString) -> crate::Error {
        crate::Error::corruption(self.path(), message)
    }
}

impl<T: IndexBuf> IndexBuf for &T {
    fn buf(&self) -> &[u8] {
        T::buf(self)
    }
    fn verify_checksum(&self, start: u64, length: u64) -> crate::Result<()> {
        T::verify_checksum(self, start, length)
    }
    fn path(&self) -> &Path {
        T::path(self)
    }
}

impl IndexBuf for Index {
    fn buf(&self) -> &[u8] {
        &self.buf
    }
    fn verify_checksum(&self, offset: u64, length: u64) -> crate::Result<()> {
        self.checksum
            .check_range(&self.buf, offset, length)
            .context(&self.path, || format!("Index path = {:?}", self.path))
    }
    fn path(&self) -> &Path {
        &self.path
    }
}

// Intentionally not inlined. This affects the "index lookup (disk, verified)"
// benchmark. It takes 74ms with this function inlined, and 61ms without.
//
// Reduce instruction count in `Index::verify_checksum`.
#[inline(never)]
fn checksum_error(checksum: &MemChecksum, offset: u64, length: u64) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "range {}..{} failed checksum check ({:?})",
            offset,
            offset + length,
            &checksum,
        ),
    ))
}

impl Index {
    /// Return a cloned [`Index`] with pending in-memory changes.
    pub fn try_clone(&self) -> crate::Result<Self> {
        self.try_clone_internal(true)
            .context("in Index::try_clone")
            .context(|| format!("  Index.path = {:?}", self.path))
    }

    /// Return a cloned [`Index`] without pending in-memory changes.
    ///
    /// This is logically equivalent to calling `clear_dirty` immediately
    /// on the result after `try_clone`, but potentially cheaper.
    pub fn try_clone_without_dirty(&self) -> crate::Result<Self> {
        self.try_clone_internal(false)
            .context("in Index::try_clone_without_dirty")
            .context(|| format!("  Index.path = {:?}", self.path))
    }

    pub(crate) fn try_clone_internal(&self, copy_dirty: bool) -> crate::Result<Index> {
        let file = match &self.file {
            Some(f) => Some(f.duplicate().context(self.path(), "cannot duplicate")?),
            None => None,
        };

        let index = if copy_dirty {
            Index {
                file,
                buf: self.buf.clone(),
                path: self.path.clone(),
                checksum_enabled: self.checksum_enabled,
                checksum_max_chain_len: self.checksum_max_chain_len,
                fsync: self.fsync,
                write: self.write,
                clean_root: self.clean_root.clone(),
                dirty_root: self.dirty_root.clone(),
                checksum: self.checksum.clone(),
                dirty_keys: self.dirty_keys.clone(),
                dirty_ext_keys: self.dirty_ext_keys.clone(),
                dirty_leafs: self.dirty_leafs.clone(),
                dirty_links: self.dirty_links.clone(),
                dirty_radixes: self.dirty_radixes.clone(),
                key_buf: self.key_buf.clone(),
                #[cfg(test)]
                fail_on_flush: self.fail_on_flush,
            }
        } else {
            Index {
                file,
                buf: self.buf.clone(),
                path: self.path.clone(),
                checksum_enabled: self.checksum_enabled,
                checksum_max_chain_len: self.checksum_max_chain_len,
                fsync: self.fsync,
                write: self.write,
                clean_root: self.clean_root.clone(),
                dirty_root: self.clean_root.clone(),
                checksum: self.checksum.clone(),
                dirty_keys: Vec::new(),
                dirty_ext_keys: Vec::new(),
                dirty_leafs: Vec::new(),
                dirty_links: Vec::new(),
                dirty_radixes: if self.clean_root.radix_offset.is_dirty() {
                    // See `clear_dirty` for this special case.
                    vec![MemRadix::default()]
                } else {
                    Vec::new()
                },
                key_buf: self.key_buf.clone(),
                #[cfg(test)]
                fail_on_flush: self.fail_on_flush,
            }
        };

        Ok(index)
    }

    /// Get metadata attached to the root node. This is what previously set by
    /// [Index::set_meta].
    pub fn get_meta(&self) -> &[u8] {
        &self.dirty_root.meta
    }

    /// Get metadata attached to the root node at file open time. This is what
    /// stored on the filesystem at the index open time, not affected by
    /// [`Index::set_meta`].
    pub fn get_original_meta(&self) -> &[u8] {
        &self.clean_root.meta
    }

    /// Set metadata attached to the root node. Will be written at
    /// [`Index::flush`] time.
    pub fn set_meta<B: AsRef<[u8]>>(&mut self, meta: B) {
        self.dirty_root.meta = meta.as_ref().to_vec().into_boxed_slice()
    }

    /// Remove dirty (in-memory) state. Restore the [`Index`] to the state as
    /// if it's just loaded from disk without modifications.
    pub fn clear_dirty(&mut self) {
        self.dirty_root = self.clean_root.clone();
        self.dirty_radixes.clear();
        if self.dirty_root.radix_offset.is_dirty() {
            // In case the disk buffer is empty, a "dirty radix" entry
            // is created automatically. Check OpenOptions::open for
            // details.
            assert_eq!(
                self.dirty_root.radix_offset,
                RadixOffset::from_dirty_index(0)
            );
            self.dirty_radixes.push(MemRadix::default());
        }
        self.dirty_leafs.clear();
        self.dirty_links.clear();
        self.dirty_keys.clear();
        self.dirty_ext_keys.clear();
    }

    /// Flush changes to disk.
    ///
    /// Take the file lock when writing.
    ///
    /// Return 0 if nothing needs to be written. Otherwise return the new file
    /// length on success. Return [`io::ErrorKind::PermissionDenied`] if the file
    /// was marked read-only at open time.
    ///
    /// The new file length can be used to obtain the exact same view of the
    /// index as it currently is. That means, other changes to the indexes won't
    /// be "combined" during flush. For example, given the following events
    /// happened in order:
    /// - Open. Get Index X.
    /// - Open using the same arguments. Get Index Y.
    /// - Write key "p" to X.
    /// - Write key "q" to Y.
    /// - Flush X. Get new length LX.
    /// - Flush Y. Get new length LY.
    /// - Open using LY as `logical_len`. Get Index Z.
    ///
    /// Then key "p" does not exist in Z. This allows some advanced use cases.
    /// On the other hand, if "merging changes" is the desired behavior, the
    /// caller needs to take another lock, re-instantiate [`Index`] and re-insert
    /// keys.
    ///
    /// For in-memory-only indexes, this function does nothing and returns 0,
    /// unless read-only was set at open time.
    ///
    /// If `flush` fails, the index could still be queried and flush again.
    pub fn flush(&mut self) -> crate::Result<u64> {
        let result: crate::Result<_> = (|| {
            let span = debug_span!("Index::flush", path = self.path.to_string_lossy().as_ref());
            let _guard = span.enter();

            if self.write == Some(false) {
                return Err(crate::Error::path(
                    self.path(),
                    "cannot flush: Index opened with read-only mode",
                ));
            }
            if self.file.is_none() {
                // Why is this Ok, not Err?
                //
                // An in-memory Index does not share data with anybody else,
                // therefore no need to flush. In other words, whether flush
                // happens or not does not change the result of other APIs on
                // this Index instance.
                //
                // Another way to think about it, an in-memory Index is similar
                // to a private anonymous mmap, and msync on that mmap would
                // succeed.
                return Ok(0);
            }

            let old_len = self.buf.len() as u64;
            let mut new_len = old_len;
            if self.dirty_root == self.clean_root && !self.dirty_root.radix_offset.is_dirty() {
                // Nothing changed
                return Ok(new_len);
            }

            // Critical section: need write lock
            {
                let mut offset_map = OffsetMap::empty_for_index(self);
                let estimated_dirty_bytes = self.dirty_links.len() * 50;
                let path = self.path.clone(); // for error messages; and make the borrowck happy.
                let mut lock = ScopedFileLock::new(self.file.as_mut().unwrap(), true)
                    .context(&path, "cannot lock")?;
                let len = lock
                    .as_mut()
                    .seek(SeekFrom::End(0))
                    .context(&path, "cannot seek to end")?;

                test_only_fail_point!(self.fail_on_flush == 1);
                if len < old_len {
                    let message = format!(
                        "on-disk index is unexpectedly smaller ({} bytes) than its previous version ({} bytes)",
                        len, old_len
                    );
                    // This is not a "corruption" - something has truncated the
                    // file, potentially recreating it. We haven't checked the
                    // new content, so it's not considered as "data corruption".
                    // TODO: Review this decision.
                    let err = crate::Error::path(&path, message);
                    return Err(err);
                }

                let mut buf = Vec::with_capacity(estimated_dirty_bytes);

                // Write in the following order:
                // header, keys, links, leafs, radixes, root.
                // Latter entries depend on former entries.

                if len == 0 {
                    buf.write_all(&[TYPE_HEAD]).infallible()?;
                }

                for (i, entry) in self.dirty_keys.iter().enumerate() {
                    if !entry.is_unused() {
                        let offset = buf.len() as u64 + len;
                        offset_map.key_map[i] = offset;
                        entry.write_to(&mut buf, &offset_map).infallible()?;
                    };
                }

                // Inlined leafs. They might affect ExtKeys and Links. Need to write first.
                for i in 0..self.dirty_leafs.len() {
                    let entry = self.dirty_leafs.get_mut(i).unwrap();
                    let offset = buf.len() as u64 + len;
                    if !entry.is_unused()
                        && entry.maybe_write_inline_to(
                            &mut buf,
                            &self.buf,
                            len,
                            &mut self.dirty_ext_keys,
                            &mut self.dirty_links,
                            &mut offset_map,
                        )?
                    {
                        offset_map.leaf_map[i] = offset;
                        entry.mark_unused();
                    }
                }

                for (i, entry) in self.dirty_ext_keys.iter().enumerate() {
                    if !entry.is_unused() {
                        let offset = buf.len() as u64 + len;
                        offset_map.ext_key_map[i] = offset;
                        entry.write_to(&mut buf, &offset_map).infallible()?;
                    }
                }

                for (i, entry) in self.dirty_links.iter().enumerate() {
                    if !entry.is_unused() {
                        let offset = buf.len() as u64 + len;
                        offset_map.link_map[i] = offset;
                        entry.write_to(&mut buf, &offset_map).infallible()?;
                    }
                }

                // Non-inlined leafs.
                for (i, entry) in self.dirty_leafs.iter().enumerate() {
                    if !entry.is_unused() {
                        let offset = buf.len() as u64 + len;
                        offset_map.leaf_map[i] = offset;
                        entry
                            .write_noninline_to(&mut buf, &offset_map)
                            .infallible()?;
                    }
                }

                // Write Radix entries in reversed order since former ones might refer to latter ones.
                for (i, entry) in self.dirty_radixes.iter().rev().enumerate() {
                    let offset = buf.len() as u64 + len;
                    entry.write_to(&mut buf, &offset_map).infallible()?;
                    offset_map.radix_map[i] = offset;
                }

                // Write Root.
                let root_len = self
                    .dirty_root
                    .write_to(&mut buf, &offset_map)
                    .infallible()?;

                // Update and write Checksum if it's enabled.
                let mut new_checksum = self.checksum.clone();

                test_only_fail_point!(self.fail_on_flush == 2);
                let checksum_len = if self.checksum_enabled {
                    new_checksum
                        .update(&self.buf, lock.as_mut(), len, &buf)
                        .context(&path, "cannot read and update checksum")?;
                    // Optionally merge the checksum entry for optimization.
                    if self.checksum_max_chain_len > 0
                        && new_checksum.chain_len >= self.checksum_max_chain_len
                    {
                        new_checksum.flatten();
                    }
                    new_checksum.write_to(&mut buf, &offset_map).infallible()?
                } else {
                    assert!(!self.checksum.is_enabled());
                    0
                };
                write_reversed_vlq(&mut buf, root_len + checksum_len).infallible()?;

                new_len = buf.len() as u64 + len;

                test_only_fail_point!(self.fail_on_flush == 3);
                lock.as_mut()
                    .seek(SeekFrom::Start(len))
                    .context(&path, "cannot seek")?;

                test_only_fail_point!(self.fail_on_flush == 4);
                lock.as_mut()
                    .write_all(&buf)
                    .context(&path, "cannot write new data to index")?;

                test_only_fail_point!(self.fail_on_flush == 5);
                if self.fsync || config::get_global_fsync() {
                    lock.as_mut().sync_all().context(&path, "cannot sync")?;
                }

                // Remap and update root since length has changed
                test_only_fail_point!(self.fail_on_flush == 6);
                let bytes = mmap_bytes(lock.as_ref(), None).context(&path, "cannot mmap")?;

                // 'path' should not have changed.
                debug_assert_eq!(&self.path, &path);

                // This is to workaround the borrow checker.
                let this = SimpleIndexBuf(&bytes, &path);

                // Sanity check - the length should be expected. Otherwise, the lock
                // is somehow ineffective.
                test_only_fail_point!(self.fail_on_flush == 7);
                if bytes.len() as u64 != new_len {
                    return Err(this.corruption("file changed unexpectedly"));
                }

                // Reload root and checksum.
                test_only_fail_point!(self.fail_on_flush == 8);
                let (root, checksum) = read_root_checksum_at_end(&path, &bytes, new_len as usize)?;

                // Only mutate `self` when everything is ready, without possible IO errors
                // in remaining operations. This avoids "partial updated, inconsistent"
                // `self` state.
                debug_assert_eq!(checksum.end, new_checksum.end);
                debug_assert_eq!(&checksum.xxhash_list, &new_checksum.xxhash_list);
                self.checksum = checksum;
                self.clean_root = root;
                self.buf = bytes;
            }

            // Outside critical section
            // Drop the in-memory state only after a successful file write.
            self.clear_dirty();

            if cfg!(all(debug_assertions, test)) {
                self.verify()
                    .expect("sync() should not break checksum check");
            }

            Ok(new_len)
        })();
        result
            .context("in Index::flush")
            .context(|| format!("  Index.path = {:?}", self.path))
    }

    /// Lookup by `key`. Return [`LinkOffset`].
    ///
    /// To test if the key exists or not, use [Offset::is_null].
    /// To obtain all values, use [`LinkOffset::values`].
    pub fn get<K: AsRef<[u8]>>(&self, key: &K) -> crate::Result<LinkOffset> {
        let result: crate::Result<_> = (|| {
            let mut offset: Offset = self.dirty_root.radix_offset.into();
            let mut iter = Base16Iter::from_base256(key);

            while !offset.is_null() {
                // Read the entry at "offset"
                match offset.to_typed(self)? {
                    TypedOffset::Radix(radix) => {
                        match iter.next() {
                            None => {
                                // The key ends at this Radix entry.
                                return radix.link_offset(self);
                            }
                            Some(x) => {
                                // Follow the `x`-th child in the Radix entry.
                                offset = radix.child(self, x)?;
                            }
                        }
                    }
                    TypedOffset::Leaf(leaf) => {
                        // Meet a leaf. If key matches, return the link offset.
                        let (stored_key, link_offset) = leaf.key_and_link_offset(self)?;
                        if stored_key == key.as_ref() {
                            return Ok(link_offset);
                        } else {
                            return Ok(LinkOffset::default());
                        }
                    }
                    _ => return Err(self.corruption("unexpected type during key lookup")),
                }
            }

            // Not found
            Ok(LinkOffset::default())
        })();

        result
            .context(|| format!("in Index::get({:?})", key.as_ref()))
            .context(|| format!("  Index.path = {:?}", self.path))
    }

    /// Scan entries which match the given prefix in base16 form.
    /// Return [`RangeIter`] which allows accesses to keys and values.
    pub fn scan_prefix_base16(
        &self,
        mut base16: impl Iterator<Item = u8>,
    ) -> crate::Result<RangeIter<'_>> {
        let mut offset: Offset = self.dirty_root.radix_offset.into();
        let mut front_stack = Vec::<IterState>::new();

        while !offset.is_null() {
            // Read the entry at "offset"
            match offset.to_typed(self)? {
                TypedOffset::Radix(radix) => {
                    match base16.next() {
                        None => {
                            let start = IterState::RadixStart(radix);
                            let end = IterState::RadixEnd(radix);
                            front_stack.push(start);
                            let mut back_stack = front_stack.clone();
                            *back_stack.last_mut().unwrap() = end;
                            return Ok(RangeIter::new(self, front_stack, back_stack));
                        }
                        Some(x) => {
                            // Follow the `x`-th child in the Radix entry.
                            front_stack.push(IterState::RadixChild(radix, x));
                            offset = radix.child(self, x)?;
                        }
                    }
                }
                TypedOffset::Leaf(leaf) => {
                    // Meet a leaf. If key matches, return the link offset.
                    let eq = {
                        let (stored_key, _link_offset) = leaf.key_and_link_offset(self)?;
                        // Remaining key matches?
                        let remaining: Vec<u8> = base16.collect();
                        Base16Iter::from_base256(&stored_key)
                            .skip(front_stack.len())
                            .take(remaining.len())
                            .eq(remaining.iter().cloned())
                    };
                    if eq {
                        let start = IterState::LeafStart(leaf);
                        let end = IterState::LeafEnd(leaf);
                        front_stack.push(start);
                        let mut back_stack = front_stack.clone();
                        *back_stack.last_mut().unwrap() = end;
                        return Ok(RangeIter::new(self, front_stack, back_stack));
                    } else {
                        return Ok(RangeIter::new(self, front_stack.clone(), front_stack));
                    };
                }
                _ => return Err(self.corruption("unexpected type during prefix scan")),
            }
        }

        // Not found
        Ok(RangeIter::new(self, front_stack.clone(), front_stack))
    }

    /// Scan entries which match the given prefix in base256 form.
    /// Return [`RangeIter`] which allows accesses to keys and values.
    pub fn scan_prefix<B: AsRef<[u8]>>(&self, prefix: B) -> crate::Result<RangeIter<'_>> {
        self.scan_prefix_base16(Base16Iter::from_base256(&prefix))
            .context(|| format!("in Index::scan_prefix({:?})", prefix.as_ref()))
            .context(|| format!("  Index.path = {:?}", self.path))
    }

    /// Scan entries which match the given prefix in hex form.
    /// Return [`RangeIter`] which allows accesses to keys and values.
    pub fn scan_prefix_hex<B: AsRef<[u8]>>(&self, prefix: B) -> crate::Result<RangeIter<'_>> {
        // Invalid hex chars will be caught by `radix.child`
        let base16 = prefix.as_ref().iter().cloned().map(single_hex_to_base16);
        self.scan_prefix_base16(base16)
            .context(|| format!("in Index::scan_prefix_hex({:?})", prefix.as_ref()))
            .context(|| format!("  Index.path = {:?}", self.path))
    }

    /// Scans entries whose keys are within the given range.
    ///
    /// Returns a double-ended iterator, which provides accesses to keys and
    /// values.
    pub fn range<'a>(&self, range: impl RangeBounds<&'a [u8]>) -> crate::Result<RangeIter<'_>> {
        let is_empty_range = match (range.start_bound(), range.end_bound()) {
            (Included(start), Included(end)) => start > end,
            (Included(start), Excluded(end)) => start > end,
            (Excluded(start), Included(end)) => start > end,
            (Excluded(start), Excluded(end)) => start >= end,
            (Unbounded, _) | (_, Unbounded) => false,
        };

        if is_empty_range {
            // `BTreeSet::range` panics in this case. Match its behavior.
            panic!("range start is greater than range end");
        }

        let result: crate::Result<_> = (|| {
            let front_stack = self.iter_stack_by_bound(range.start_bound(), Front)?;
            let back_stack = self.iter_stack_by_bound(range.end_bound(), Back)?;
            Ok(RangeIter::new(self, front_stack, back_stack))
        })();

        result
            .context(|| {
                format!(
                    "in Index::range({:?} to {:?})",
                    range.start_bound(),
                    range.end_bound()
                )
            })
            .context(|| format!("  Index.path = {:?}", self.path))
    }

    /// Insert a key-value pair. The value will be the head of the linked list.
    /// That is, `get(key).values().first()` will return the newly inserted
    /// value.
    pub fn insert<K: AsRef<[u8]>>(&mut self, key: &K, value: u64) -> crate::Result<()> {
        self.insert_advanced(InsertKey::Embed(key.as_ref()), InsertValue::Prepend(value))
            .context(|| format!("in Index::insert(key={:?}, value={})", key.as_ref(), value))
            .context(|| format!("  Index.path = {:?}", self.path))
    }

    /// Remove all values associated with the given key.
    pub fn remove(&mut self, key: impl AsRef<[u8]>) -> crate::Result<()> {
        // NOTE: The implementation detail does not remove radix entries to
        // reclaim space or improve lookup performance. For example, inserting
        // "abcde" to an empty index, following by deleting it will still
        // require O(5) jumps looking up "abcde".
        let key = key.as_ref();
        self.insert_advanced(InsertKey::Embed(key), InsertValue::Tombstone)
            .context(|| format!("in Index::remove(key={:?})", key))
            .context(|| format!("  Index.path = {:?}", self.path))
    }

    /// Remove all values associated with all keys with the given prefix.
    pub fn remove_prefix(&mut self, prefix: impl AsRef<[u8]>) -> crate::Result<()> {
        // NOTE: See "remove". The implementation detail does not optimize
        // for space or lookup performance.
        let prefix = prefix.as_ref();
        self.insert_advanced(InsertKey::Embed(prefix), InsertValue::TombstonePrefix)
            .context(|| format!("in Index::remove_prefix(prefix={:?})", prefix))
            .context(|| format!("  Index.path = {:?}", self.path))
    }

    /// Update the linked list for a given key.
    ///
    /// If `link` is None, behave like `insert`. Otherwise, ignore the existing
    /// values `key` mapped to, create a new link entry that chains to the given
    /// [`LinkOffset`].
    ///
    /// `key` could be a reference, or an embedded value. See [`InsertKey`] for
    /// details.
    ///
    /// This is a low-level API.
    pub fn insert_advanced(&mut self, key: InsertKey, value: InsertValue) -> crate::Result<()> {
        let mut offset: Offset = self.dirty_root.radix_offset.into();
        let mut step = 0;
        let (key, key_buf_offset) = match key {
            InsertKey::Embed(k) => (k, None),
            InsertKey::Reference((start, len)) => {
                let key = match self.key_buf.as_ref().slice(start, len) {
                    Some(k) => k,
                    None => {
                        return Err(
                            self.corruption("key buffer is invalid when inserting referred keys")
                        );
                    }
                };
                // UNSAFE NOTICE: `key` is valid as long as `self.key_buf` is valid. `self.key_buf`
                // won't be changed. So `self` can still be mutable without a read-only
                // relationship with `key`.
                let detached_key = unsafe { &*(key as *const [u8]) };
                (detached_key, Some((start, len)))
            }
        };
        let mut iter = Base16Iter::from_base256(&key);

        let mut last_radix = RadixOffset::default();
        let mut last_child = 0u8;

        loop {
            match offset.to_typed(&*self)? {
                TypedOffset::Radix(radix) => {
                    // Copy radix entry since we must modify it.
                    let radix = radix.copy(self)?;
                    offset = radix.into();

                    if step == 0 {
                        self.dirty_root.radix_offset = radix;
                    } else {
                        last_radix.set_child(self, last_child, offset);
                    }

                    last_radix = radix;

                    match iter.next() {
                        None => {
                            // "key" is shorter than existing ones. No need to create a new key.
                            // For example, insert "a", when root.radix is {'a': {'b': { ... }}}.
                            let old_link_offset = radix.link_offset(self)?;
                            let new_link_offset = match value {
                                InsertValue::Prepend(value) => old_link_offset.create(self, value),
                                InsertValue::PrependReplace(value, link_offset) => {
                                    link_offset.create(self, value)
                                }
                                InsertValue::Tombstone => LinkOffset::default(),
                                InsertValue::TombstonePrefix => {
                                    radix.set_all_to_null(self);
                                    return Ok(());
                                }
                            };
                            radix.set_link(self, new_link_offset);
                            return Ok(());
                        }
                        Some(x) => {
                            let next_offset = radix.child(self, x)?;
                            if next_offset.is_null() {
                                // "key" is longer than existing ones. Create key and leaf entries.
                                // For example, insert "abcd", when root.radix is {'a': {}}.
                                let new_link_offset = match value {
                                    InsertValue::Prepend(value) => {
                                        LinkOffset::default().create(self, value)
                                    }
                                    InsertValue::PrependReplace(value, link_offset) => {
                                        link_offset.create(self, value)
                                    }
                                    InsertValue::Tombstone | InsertValue::TombstonePrefix => {
                                        // No need to create a key.
                                        radix.set_child(self, x, Offset::null());
                                        return Ok(());
                                    }
                                };
                                let key_offset = self.create_key(key, key_buf_offset);
                                let leaf_offset =
                                    LeafOffset::create(self, new_link_offset, key_offset);
                                radix.set_child(self, x, leaf_offset.into());
                                return Ok(());
                            } else {
                                offset = next_offset;
                                last_child = x;
                            }
                        }
                    }
                }
                TypedOffset::Leaf(leaf) => {
                    let (old_key, old_link_offset) = {
                        let (old_key, link_offset) = leaf.key_and_link_offset(self)?;
                        // Detach "old_key" from "self".
                        // About safety: This is to avoid a memory copy / allocation.
                        // `old_key` are only valid before `dirty_*keys` being resized.
                        // `old_iter` (used by `split_leaf`) and `old_key` are not used
                        // after creating a key. So it's safe to not copy it.
                        let detached_key = unsafe { &*(old_key as *const [u8]) };
                        (detached_key, link_offset)
                    };
                    let matched = if let InsertValue::TombstonePrefix = value {
                        // Only test the prefix of old_key.
                        old_key.get(..key.len()) == Some(key)
                    } else {
                        old_key == key
                    };
                    if matched {
                        // Key matched. Need to copy leaf entry for modification, except for
                        // deletion.
                        let new_link_offset = match value {
                            InsertValue::Prepend(value) => old_link_offset.create(self, value),
                            InsertValue::PrependReplace(value, link_offset) => {
                                link_offset.create(self, value)
                            }
                            InsertValue::Tombstone | InsertValue::TombstonePrefix => {
                                // No need to copy the leaf entry.
                                last_radix.set_child(self, last_child, Offset::null());
                                return Ok(());
                            }
                        };
                        let new_leaf_offset = leaf.set_link(self, new_link_offset)?;
                        last_radix.set_child(self, last_child, new_leaf_offset.into());
                    } else {
                        // Key mismatch. Do a leaf split unless it's a deletion.
                        let new_link_offset = match value {
                            InsertValue::Prepend(value) => {
                                LinkOffset::default().create(self, value)
                            }
                            InsertValue::PrependReplace(value, link_offset) => {
                                link_offset.create(self, value)
                            }
                            InsertValue::Tombstone | InsertValue::TombstonePrefix => return Ok(()),
                        };
                        self.split_leaf(
                            leaf,
                            old_key,
                            key.as_ref(),
                            key_buf_offset,
                            step,
                            last_radix,
                            last_child,
                            old_link_offset,
                            new_link_offset,
                        )?;
                    }
                    return Ok(());
                }
                _ => return Err(self.corruption("unexpected type during insertion")),
            }

            step += 1;
        }
    }

    /// Convert a slice to [`Bytes`].
    /// Do not copy the slice if it's from the on-disk buffer.
    pub fn slice_to_bytes(&self, slice: &[u8]) -> Bytes {
        self.buf.slice_to_bytes(slice)
    }

    /// Verify checksum for the entire on-disk buffer.
    pub fn verify(&self) -> crate::Result<()> {
        self.verify_checksum(0, self.checksum.end)
    }

    // Internal function used by [`Index::range`].
    // Calculate the [`IterState`] stack used by [`RangeIter`].
    // `side` is the side of the `bound`, starting side of the iteration,
    // the opposite of "towards" side.
    fn iter_stack_by_bound(
        &self,
        bound: Bound<&&[u8]>,
        side: Side,
    ) -> crate::Result<Vec<IterState>> {
        let root_radix = self.dirty_root.radix_offset;
        let (inclusive, mut base16iter) = match bound {
            Unbounded => {
                return Ok(match side {
                    Front => vec![IterState::RadixStart(root_radix)],
                    Back => vec![IterState::RadixEnd(root_radix)],
                });
            }
            Included(ref key) => (true, Base16Iter::from_base256(key)),
            Excluded(ref key) => (false, Base16Iter::from_base256(key)),
        };

        let mut offset: Offset = root_radix.into();
        let mut stack = Vec::<IterState>::new();

        while !offset.is_null() {
            match offset.to_typed(self)? {
                TypedOffset::Radix(radix) => match base16iter.next() {
                    None => {
                        // The key ends at this Radix entry.
                        let state = IterState::RadixLeaf(radix);
                        let state = if inclusive {
                            state.step(side).unwrap()
                        } else {
                            state
                        };
                        stack.push(state);
                        return Ok(stack);
                    }
                    Some(x) => {
                        // Follow the `x`-th child in the Radix entry.
                        stack.push(IterState::RadixChild(radix, x));
                        offset = radix.child(self, x)?;
                    }
                },
                TypedOffset::Leaf(leaf) => {
                    let stored_cmp_key = {
                        let (stored_key, _link_offset) = leaf.key_and_link_offset(self)?;
                        Base16Iter::from_base256(&stored_key)
                            .skip(stack.len())
                            .cmp(base16iter)
                    };
                    let state = IterState::Leaf(leaf);
                    let state = match (stored_cmp_key, side, inclusive) {
                        (Equal, _, true) | (Less, Back, _) | (Greater, Front, _) => {
                            state.step(side).unwrap()
                        }
                        (Equal, _, false) | (Greater, Back, _) | (Less, Front, _) => state,
                    };
                    stack.push(state);
                    return Ok(stack);
                }
                _ => return Err(self.corruption("unexpected type following prefix")),
            }
        }

        // Prefix does not exist. The stack ends with a RadixChild state that
        // points to nothing.
        Ok(stack)
    }

    #[allow(clippy::too_many_arguments)]
    /// Split a leaf entry. Separated from `insert_advanced` to make `insert_advanced`
    /// shorter.  The parameters are internal states inside `insert_advanced`. Calling this
    /// from other functions makes less sense.
    #[inline]
    fn split_leaf(
        &mut self,
        old_leaf_offset: LeafOffset,
        old_key: &[u8],
        new_key: &[u8],
        key_buf_offset: Option<(u64, u64)>,
        step: usize,
        radix_offset: RadixOffset,
        child: u8,
        old_link_offset: LinkOffset,
        new_link_offset: LinkOffset,
    ) -> crate::Result<()> {
        // This is probably the most complex part. Here are some explanation about input parameters
        // and what this function is supposed to do for some cases:
        //
        // Input parameters are marked using `*`:
        //
        //      Offset            | Content
        //      root_radix        | Radix(child1: radix1, ...)         \
        //      radix1            | Radix(child2: radix2, ...)         |> steps
        //      ...               | ...                                | (for skipping check
        //      *radix_offset*    | Radix(*child*: *leaf_offset*, ...) /  of prefix in keys)
        //      *old_leaf_offset* | Leaf(link_offset: *old_link_offset*, ...)
        //      *new_link_offset* | Link(...)
        //
        //      old_* are redundant, but they are pre-calculated by the caller. So just reuse them.
        //
        // Here are 3 kinds of examples (Keys are embed in Leaf for simplicity):
        //
        // Example 1. old_key = "1234"; new_key = "1278".
        //
        //      Offset | Before                | After
        //           A | Radix(1: B)           | Radix(1: C)
        //           B | Leaf("1234", Link: X) | Leaf("1234", Link: X)
        //           C |                       | Radix(2: E)
        //           D |                       | Leaf("1278")
        //           E |                       | Radix(3: B, 7: D)
        //
        // Example 2. old_key = "1234", new_key = "12". No need for a new leaf entry:
        //
        //      Offset | Before                | After
        //           A | Radix(1: B)           | Radix(1: C)
        //           B | Leaf("1234", Link: X) | Leaf("1234", Link: X)
        //           C |                       | Radix(2: B, Link: Y)
        //
        // Example 3. old_key = "12", new_key = "1234". Need new leaf. Old leaf is not needed.
        //
        //      Offset | Before              | After
        //           A | Radix(1: B)         | Radix(1: C)
        //           B | Leaf("12", Link: X) | Leaf("12", Link: X) # not used
        //           C |                     | Radix(2: E, Link: X)
        //           D |                     | Leaf("1234", Link: Y)
        //           E |                     | Radix(3: D)

        // UNSAFE NOTICE: Read the "UNSAFE NOTICE" inside `insert_advanced` to learn more.
        // Basically, `old_iter` is only guaranteed available if there is no insertion to
        // `self.dirty_keys` or `self.dirty_ext_keys`. That's true here since we won't read
        // `old_iter` after creating new keys. But be aware of the constraint when modifying the
        // code.
        let mut old_iter = Base16Iter::from_base256(&old_key).skip(step);
        let mut new_iter = Base16Iter::from_base256(&new_key).skip(step);

        let mut last_radix_offset = radix_offset;
        let mut last_radix_child = child;

        let mut completed = false;

        loop {
            let b1 = old_iter.next();
            let b2 = new_iter.next();

            let mut radix = MemRadix::default();

            if let Some(b1) = b1 {
                // Initial value for the b1-th child. Could be rewritten by
                // "set_radix_entry_child" in the next loop iteration.
                radix.offsets[b1 as usize] = old_leaf_offset.into();
            } else {
                // Example 3. old_key is a prefix of new_key. A leaf is still needed.
                // The new leaf will be created by the next "if" block.
                old_leaf_offset.mark_unused(self);
                radix.link_offset = old_link_offset;
            }

            match b2 {
                None => {
                    // Example 2. new_key is a prefix of old_key. A new leaf is not needed.
                    radix.link_offset = new_link_offset;
                    completed = true;
                }
                Some(b2v) if b1 != b2 => {
                    // Example 1 and Example 3. A new leaf is needed.
                    let new_key_offset = self.create_key(new_key, key_buf_offset);
                    let new_leaf_offset = LeafOffset::create(self, new_link_offset, new_key_offset);
                    radix.offsets[b2v as usize] = new_leaf_offset.into();
                    completed = true;
                }
                _ => {}
            }

            // Create the Radix entry, and connect it to the parent entry.
            let offset = RadixOffset::create(self, radix);
            last_radix_offset.set_child(self, last_radix_child, offset.into());

            if completed {
                break;
            }

            debug_assert!(b1 == b2);
            last_radix_offset = offset;
            last_radix_child = b2.unwrap();
        }

        Ok(())
    }

    /// Create a key (if key_buf_offset is None) or ext key (if key_buf_offset is set) entry.
    #[inline]
    fn create_key(&mut self, key: &[u8], key_buf_offset: Option<(u64, u64)>) -> Offset {
        match key_buf_offset {
            None => KeyOffset::create(self, key).into(),
            Some((start, len)) => ExtKeyOffset::create(self, start, len).into(),
        }
    }
}

/// Specify value to insert. Used by `insert_advanced`.
#[derive(Copy, Clone)]
pub enum InsertValue {
    /// Insert as a head of the existing linked list.
    Prepend(u64),

    /// Replace the linked list. Then insert as a head.
    PrependReplace(u64, LinkOffset),

    /// Effectively delete associated values for the specified key.
    Tombstone,

    /// Effectively delete associated values for all keys starting with the
    /// prefix.
    TombstonePrefix,
}

/// Debug Formatter
impl Debug for Offset {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        if self.is_null() {
            write!(f, "None")
        } else if self.is_dirty() {
            let path = Path::new("<dummy>");
            let dummy = SimpleIndexBuf(b"", path);
            match self.to_typed(dummy).unwrap() {
                TypedOffset::Radix(x) => x.fmt(f),
                TypedOffset::Leaf(x) => x.fmt(f),
                TypedOffset::Link(x) => x.fmt(f),
                TypedOffset::Key(x) => x.fmt(f),
                TypedOffset::ExtKey(x) => x.fmt(f),
                TypedOffset::Checksum(x) => x.fmt(f),
            }
        } else {
            write!(f, "Disk[{}]", self.0)
        }
    }
}

impl Debug for MemRadix {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(f, "Radix {{ link: {:?}", self.link_offset)?;
        for (i, v) in self.offsets.iter().cloned().enumerate() {
            if !v.is_null() {
                write!(f, ", {}: {:?}", i, v)?;
            }
        }
        write!(f, " }}")
    }
}

impl Debug for MemLeaf {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        if self.is_unused() {
            write!(f, "Leaf (unused)")
        } else {
            write!(
                f,
                "Leaf {{ key: {:?}, link: {:?} }}",
                self.key_offset, self.link_offset
            )
        }
    }
}

impl Debug for MemLink {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "Link {{ value: {}, next: {:?} }}",
            self.value, self.next_link_offset
        )
    }
}

impl Debug for MemKey {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        if self.is_unused() {
            write!(f, "Key (unused)")
        } else {
            write!(f, "Key {{ key:")?;
            for byte in self.key.iter() {
                write!(f, " {:X}", byte)?;
            }
            write!(f, " }}")
        }
    }
}

impl Debug for MemExtKey {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        if self.is_unused() {
            write!(f, "ExtKey (unused)")
        } else {
            write!(f, "ExtKey {{ start: {}, len: {} }}", self.start, self.len)
        }
    }
}

impl Debug for MemRoot {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        if self.meta.is_empty() {
            write!(f, "Root {{ radix: {:?} }}", self.radix_offset)
        } else {
            write!(
                f,
                "Root {{ radix: {:?}, meta: {:?} }}",
                self.radix_offset, self.meta
            )
        }
    }
}
impl Debug for MemChecksum {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "Checksum {{ start: {}, end: {}, chunk_size_logarithm: {}, checksums.len(): {} }}",
            self.start,
            self.end,
            self.chunk_size_logarithm,
            self.xxhash_list_to_write().len(),
        )
    }
}

impl Debug for Index {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        writeln!(
            f,
            "Index {{ len: {}, root: {:?} }}",
            self.buf.len(),
            self.dirty_root.radix_offset
        )?;

        // On-disk entries
        let offset_map = OffsetMap::default();
        let mut buf = Vec::with_capacity(self.buf.len());
        buf.push(TYPE_HEAD);
        let mut root_offset = 0;
        loop {
            let i = buf.len();
            if i >= self.buf.len() {
                break;
            }
            write!(f, "Disk[{}]: ", i)?;
            let type_int = self.buf[i];
            let i = i as u64;
            match type_int {
                TYPE_RADIX => {
                    let e = MemRadix::read_from(self, i).expect("read");
                    e.write_to(&mut buf, &offset_map).expect("write");
                    writeln!(f, "{:?}", e)?;
                }
                TYPE_LEAF => {
                    let e = MemLeaf::read_from(self, i).expect("read");
                    e.write_noninline_to(&mut buf, &offset_map).expect("write");
                    writeln!(f, "{:?}", e)?;
                }
                TYPE_INLINE_LEAF => {
                    let e = MemLeaf::read_from(self, i).expect("read");
                    writeln!(f, "Inline{:?}", e)?;
                    // Just skip the type int byte so we can parse inlined structures.
                    buf.push(TYPE_INLINE_LEAF);
                }
                TYPE_LINK => {
                    let e = MemLink::read_from(self, i).unwrap();
                    e.write_to(&mut buf, &offset_map).expect("write");
                    writeln!(f, "{:?}", e)?;
                }
                TYPE_KEY => {
                    let e = MemKey::read_from(self, i).expect("read");
                    e.write_to(&mut buf, &offset_map).expect("write");
                    writeln!(f, "{:?}", e)?;
                }
                TYPE_EXT_KEY => {
                    let e = MemExtKey::read_from(self, i).expect("read");
                    e.write_to(&mut buf, &offset_map).expect("write");
                    writeln!(f, "{:?}", e)?;
                }
                TYPE_ROOT => {
                    root_offset = i as usize;
                    let e = MemRoot::read_from(self, i).expect("read").0;
                    e.write_to(&mut buf, &offset_map).expect("write");
                    // Preview the next entry. If it's not Checksum, then it's in "legacy"
                    // format and we need to write "reversed_vlq" length of the Root entry
                    // immediately.
                    if self.buf.get(buf.len()) != Some(&TYPE_CHECKSUM)
                        || MemChecksum::read_from(&self, buf.len() as u64).is_err()
                    {
                        let root_len = buf.len() - root_offset;
                        write_reversed_vlq(&mut buf, root_len).expect("write");
                    }
                    writeln!(f, "{:?}", e)?;
                }
                TYPE_CHECKSUM => {
                    let e = MemChecksum::read_from(&self, i).expect("read").0;
                    e.write_to(&mut buf, &offset_map).expect("write");
                    let root_checksum_len = buf.len() - root_offset;
                    let vlq_start = buf.len();
                    write_reversed_vlq(&mut buf, root_checksum_len).expect("write");
                    let vlq_end = buf.len();
                    debug_assert_eq!(
                        &buf[vlq_start..vlq_end],
                        &self.buf[vlq_start..vlq_end],
                        "reversed vlq should match (root+checksum len: {})",
                        root_checksum_len
                    );
                    writeln!(f, "{:?}", e)?;
                }
                _ => {
                    writeln!(f, "Broken Data!")?;
                    break;
                }
            }
        }

        if buf.len() > 1 && self.buf[..] != buf[..] {
            debug_assert_eq!(&self.buf[..], &buf[..]);
            return writeln!(f, "Inconsistent Data!");
        }

        // In-memory entries
        for (i, e) in self.dirty_radixes.iter().enumerate() {
            write!(f, "Radix[{}]: ", i)?;
            writeln!(f, "{:?}", e)?;
        }

        for (i, e) in self.dirty_leafs.iter().enumerate() {
            write!(f, "Leaf[{}]: ", i)?;
            writeln!(f, "{:?}", e)?;
        }

        for (i, e) in self.dirty_links.iter().enumerate() {
            write!(f, "Link[{}]: ", i)?;
            writeln!(f, "{:?}", e)?;
        }

        for (i, e) in self.dirty_keys.iter().enumerate() {
            write!(f, "Key[{}]: ", i)?;
            writeln!(f, "{:?}", e)?;
        }

        for (i, e) in self.dirty_ext_keys.iter().enumerate() {
            writeln!(f, "ExtKey[{}]: {:?}", i, e)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::collections::HashMap;
    use std::fs::File;

    use quickcheck::quickcheck;
    use tempfile::tempdir;

    use super::InsertValue::PrependReplace;
    use super::*;

    fn open_opts() -> OpenOptions {
        let mut opts = OpenOptions::new();
        opts.checksum_chunk_size_logarithm(4);
        opts
    }

    fn in_memory_index() -> Index {
        OpenOptions::new().create_in_memory().unwrap()
    }

    #[test]
    fn test_scan_prefix() {
        let dir = tempdir().unwrap();
        let mut index = open_opts().open(dir.path().join("a")).unwrap();
        let keys: Vec<&[u8]> = vec![b"01", b"02", b"03", b"031", b"0410", b"042", b"05000"];
        for (i, key) in keys.iter().enumerate() {
            index.insert(key, i as u64).unwrap();
        }

        // Return keys with the given prefix. Also verify LinkOffsets.
        let scan_keys = |prefix: &[u8]| -> Vec<Vec<u8>> {
            let iter = index.scan_prefix(prefix).unwrap();
            iter_to_keys(&index, &keys, &iter)
        };

        assert_eq!(scan_keys(b"01"), vec![b"01"]);
        assert!(scan_keys(b"010").is_empty());
        assert_eq!(scan_keys(b"02"), vec![b"02"]);
        assert!(scan_keys(b"020").is_empty());
        assert_eq!(scan_keys(b"03"), vec![&b"03"[..], b"031"]);
        assert_eq!(scan_keys(b"031"), vec![b"031"]);
        assert!(scan_keys(b"032").is_empty());
        assert_eq!(scan_keys(b"04"), vec![&b"0410"[..], b"042"]);
        assert_eq!(scan_keys(b"041"), vec![b"0410"]);
        assert_eq!(scan_keys(b"0410"), vec![b"0410"]);
        assert!(scan_keys(b"04101").is_empty());
        assert!(scan_keys(b"0412").is_empty());
        assert_eq!(scan_keys(b"042"), vec![b"042"]);
        assert!(scan_keys(b"0421").is_empty());
        assert_eq!(scan_keys(b"05"), vec![b"05000"]);
        assert_eq!(scan_keys(b"0500"), vec![b"05000"]);
        assert_eq!(scan_keys(b"05000"), vec![b"05000"]);
        assert!(scan_keys(b"051").is_empty());
        assert_eq!(scan_keys(b"0"), keys);
        assert_eq!(scan_keys(b""), keys);
        assert!(scan_keys(b"1").is_empty());

        // 0x30 = b'0'
        assert_eq!(index.scan_prefix_hex(b"30").unwrap().count(), keys.len());
        assert_eq!(index.scan_prefix_hex(b"3").unwrap().count(), keys.len());
        assert_eq!(index.scan_prefix_hex(b"31").unwrap().count(), 0);
    }

    #[test]
    fn test_remove() {
        let dir = tempdir().unwrap();
        let mut index = open_opts().open(dir.path().join("a")).expect("open");

        // Removing keys on an empty index should not create new entries.
        let text = format!("{:?}", &index);
        index.remove("").unwrap();
        index.remove("a").unwrap();
        assert_eq!(text, format!("{:?}", &index));

        index.insert(b"abc", 42).unwrap();
        index.insert(b"abc", 43).unwrap();
        index.insert(b"abxyz", 44).unwrap();
        index.flush().unwrap();

        // Remove known keys.
        assert_eq!(index.range(..).unwrap().count(), 2);
        index.remove(b"abxyz").unwrap();
        assert_eq!(index.range(..).unwrap().count(), 1);
        index.remove(b"abc").unwrap();
        assert_eq!(index.range(..).unwrap().count(), 0);

        // Since all entries are "dirty" in memory, removing keys should not create new entries.
        let text = format!("{:?}", &index);
        index.remove("").unwrap();
        index.remove("a").unwrap();
        index.remove("ab").unwrap();
        index.remove("abc").unwrap();
        index.remove("abcd").unwrap();
        index.remove("abcde").unwrap();
        index.remove("abcx").unwrap();
        index.remove("abcxyz").unwrap();
        index.remove("abcxyzz").unwrap();
        assert_eq!(text, format!("{:?}", &index));

        // Removal state can be saved to disk.
        index.flush().unwrap();
        let index = open_opts().open(dir.path().join("a")).expect("open");
        assert_eq!(index.range(..).unwrap().count(), 0);
    }

    #[test]
    fn test_remove_recursive() {
        let dir = tempdir().unwrap();
        let mut index = open_opts().open(dir.path().join("a")).expect("open");
        index.insert(b"abc", 42).unwrap();
        index.insert(b"abc", 42).unwrap();
        index.insert(b"abxyz1", 42).unwrap();
        index.insert(b"abxyz2", 42).unwrap();
        index.insert(b"abxyz33333", 42).unwrap();
        index.insert(b"abxyz44444", 42).unwrap();
        index.insert(b"aby", 42).unwrap();
        index.flush().unwrap();

        let mut index = open_opts().open(dir.path().join("a")).expect("open");
        let mut n = index.range(..).unwrap().count();
        index.remove_prefix(b"abxyz33333333333").unwrap(); // nothing removed
        assert_eq!(index.range(..).unwrap().count(), n);

        index.remove_prefix(b"abxyz33333").unwrap(); // exact match
        n -= 1; // abxyz33333 removed
        assert_eq!(index.range(..).unwrap().count(), n);

        index.remove_prefix(b"abxyz4").unwrap(); // prefix exact match
        n -= 1; // abxyz44444 removed
        assert_eq!(index.range(..).unwrap().count(), n);

        index.remove_prefix(b"ab").unwrap(); // prefix match
        n -= 4; // abc, aby, abxyz1, abxyz2 removed
        assert_eq!(index.range(..).unwrap().count(), n);

        let mut index = open_opts().open(dir.path().join("a")).expect("open");
        index.remove_prefix(b"").unwrap(); // remove everything
        assert_eq!(index.range(..).unwrap().count(), 0);
    }

    #[test]
    fn test_distinct_one_byte_keys() {
        let dir = tempdir().unwrap();
        let mut index = open_opts().open(dir.path().join("a")).expect("open");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 0, root: Radix[0] }\n\
             Radix[0]: Radix { link: None }\n"
        );

        index.insert(&[], 55).expect("update");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 0, root: Radix[0] }
Radix[0]: Radix { link: Link[0] }
Link[0]: Link { value: 55, next: None }
"#
        );

        index.insert(&[0x12], 77).expect("update");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 0, root: Radix[0] }
Radix[0]: Radix { link: Link[0], 1: Leaf[0] }
Leaf[0]: Leaf { key: Key[0], link: Link[1] }
Link[0]: Link { value: 55, next: None }
Link[1]: Link { value: 77, next: None }
Key[0]: Key { key: 12 }
"#
        );

        let link = index.get(&[0x12]).expect("get");
        index
            .insert_advanced(InsertKey::Embed(&[0x34]), PrependReplace(99, link))
            .expect("update");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 0, root: Radix[0] }
Radix[0]: Radix { link: Link[0], 1: Leaf[0], 3: Leaf[1] }
Leaf[0]: Leaf { key: Key[0], link: Link[1] }
Leaf[1]: Leaf { key: Key[1], link: Link[2] }
Link[0]: Link { value: 55, next: None }
Link[1]: Link { value: 77, next: None }
Link[2]: Link { value: 99, next: Link[1] }
Key[0]: Key { key: 12 }
Key[1]: Key { key: 34 }
"#
        );
    }

    #[test]
    fn test_distinct_one_byte_keys_flush() {
        let dir = tempdir().unwrap();
        let mut index = open_opts().open(dir.path().join("a")).expect("open");

        // 1st flush.
        assert_eq!(index.flush().expect("flush"), 24);
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 24, root: Disk[1] }
Disk[1]: Radix { link: None }
Disk[5]: Root { radix: Disk[1] }
Disk[8]: Checksum { start: 0, end: 8, chunk_size_logarithm: 4, checksums.len(): 1 }
"#
        );

        // Mixed on-disk and in-memory state.
        index.insert(&[], 55).expect("update");
        index.insert(&[0x12], 77).expect("update");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 24, root: Radix[0] }
Disk[1]: Radix { link: None }
Disk[5]: Root { radix: Disk[1] }
Disk[8]: Checksum { start: 0, end: 8, chunk_size_logarithm: 4, checksums.len(): 1 }
Radix[0]: Radix { link: Link[0], 1: Leaf[0] }
Leaf[0]: Leaf { key: Key[0], link: Link[1] }
Link[0]: Link { value: 55, next: None }
Link[1]: Link { value: 77, next: None }
Key[0]: Key { key: 12 }
"#
        );

        // After 2nd flush. There are 2 roots.
        let link = index.get(&[0x12]).expect("get");
        index
            .insert_advanced(InsertKey::Embed(&[0x34]), PrependReplace(99, link))
            .expect("update");
        index.flush().expect("flush");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 104, root: Disk[45] }
Disk[1]: Radix { link: None }
Disk[5]: Root { radix: Disk[1] }
Disk[8]: Checksum { start: 0, end: 8, chunk_size_logarithm: 4, checksums.len(): 1 }
Disk[24]: Key { key: 12 }
Disk[27]: Key { key: 34 }
Disk[30]: Link { value: 55, next: None }
Disk[33]: Link { value: 77, next: None }
Disk[36]: Link { value: 99, next: Disk[33] }
Disk[39]: Leaf { key: Disk[24], link: Disk[33] }
Disk[42]: Leaf { key: Disk[27], link: Disk[36] }
Disk[45]: Radix { link: Disk[30], 1: Disk[39], 3: Disk[42] }
Disk[61]: Root { radix: Disk[45] }
Disk[64]: Checksum { start: 0, end: 64, chunk_size_logarithm: 4, checksums.len(): 4 }
"#
        );
    }

    #[test]
    fn test_leaf_split() {
        let dir = tempdir().unwrap();
        let mut index = open_opts().open(dir.path().join("a")).expect("open");

        // Example 1: two keys are not prefixes of each other
        index.insert(&[0x12, 0x34], 5).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 0, root: Radix[0] }
Radix[0]: Radix { link: None, 1: Leaf[0] }
Leaf[0]: Leaf { key: Key[0], link: Link[0] }
Link[0]: Link { value: 5, next: None }
Key[0]: Key { key: 12 34 }
"#
        );
        index.insert(&[0x12, 0x78], 7).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 0, root: Radix[0] }
Radix[0]: Radix { link: None, 1: Radix[1] }
Radix[1]: Radix { link: None, 2: Radix[2] }
Radix[2]: Radix { link: None, 3: Leaf[0], 7: Leaf[1] }
Leaf[0]: Leaf { key: Key[0], link: Link[0] }
Leaf[1]: Leaf { key: Key[1], link: Link[1] }
Link[0]: Link { value: 5, next: None }
Link[1]: Link { value: 7, next: None }
Key[0]: Key { key: 12 34 }
Key[1]: Key { key: 12 78 }
"#
        );

        // Example 2: new key is a prefix of the old key
        let mut index = open_opts().open(dir.path().join("a")).expect("open");
        index.insert(&[0x12, 0x34], 5).expect("insert");
        index.insert(&[0x12], 7).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 0, root: Radix[0] }
Radix[0]: Radix { link: None, 1: Radix[1] }
Radix[1]: Radix { link: None, 2: Radix[2] }
Radix[2]: Radix { link: Link[1], 3: Leaf[0] }
Leaf[0]: Leaf { key: Key[0], link: Link[0] }
Link[0]: Link { value: 5, next: None }
Link[1]: Link { value: 7, next: None }
Key[0]: Key { key: 12 34 }
"#
        );

        // Example 3: old key is a prefix of the new key
        let mut index = open_opts().open(dir.path().join("a")).expect("open");
        index.insert(&[0x12], 5).expect("insert");
        index.insert(&[0x12, 0x78], 7).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 0, root: Radix[0] }
Radix[0]: Radix { link: None, 1: Radix[1] }
Radix[1]: Radix { link: None, 2: Radix[2] }
Radix[2]: Radix { link: Link[0], 7: Leaf[1] }
Leaf[0]: Leaf (unused)
Leaf[1]: Leaf { key: Key[1], link: Link[1] }
Link[0]: Link { value: 5, next: None }
Link[1]: Link { value: 7, next: None }
Key[0]: Key (unused)
Key[1]: Key { key: 12 78 }
"#
        );

        // Same key. Multiple values.
        let mut index = open_opts().open(dir.path().join("a")).expect("open");
        index.insert(&[0x12], 5).expect("insert");
        index.insert(&[0x12], 7).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 0, root: Radix[0] }
Radix[0]: Radix { link: None, 1: Leaf[0] }
Leaf[0]: Leaf { key: Key[0], link: Link[1] }
Link[0]: Link { value: 5, next: None }
Link[1]: Link { value: 7, next: Link[0] }
Key[0]: Key { key: 12 }
"#
        );
    }

    #[test]
    fn test_leaf_split_flush() {
        // Similar with test_leaf_split, but flush the first key before inserting the second.
        // This triggers some new code paths.
        let dir = tempdir().unwrap();
        let mut index = open_opts().open(dir.path().join("1")).expect("open");

        // Example 1: two keys are not prefixes of each other
        index.insert(&[0x12, 0x34], 5).expect("insert");
        index.flush().expect("flush");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 46, root: Disk[11] }
Disk[1]: Key { key: 12 34 }
Disk[5]: Link { value: 5, next: None }
Disk[8]: Leaf { key: Disk[1], link: Disk[5] }
Disk[11]: Radix { link: None, 1: Disk[8] }
Disk[19]: Root { radix: Disk[11] }
Disk[22]: Checksum { start: 0, end: 22, chunk_size_logarithm: 4, checksums.len(): 2 }
"#
        );
        index.insert(&[0x12, 0x78], 7).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 46, root: Radix[0] }
Disk[1]: Key { key: 12 34 }
Disk[5]: Link { value: 5, next: None }
Disk[8]: Leaf { key: Disk[1], link: Disk[5] }
Disk[11]: Radix { link: None, 1: Disk[8] }
Disk[19]: Root { radix: Disk[11] }
Disk[22]: Checksum { start: 0, end: 22, chunk_size_logarithm: 4, checksums.len(): 2 }
Radix[0]: Radix { link: None, 1: Radix[1] }
Radix[1]: Radix { link: None, 2: Radix[2] }
Radix[2]: Radix { link: None, 3: Disk[8], 7: Leaf[0] }
Leaf[0]: Leaf { key: Key[0], link: Link[0] }
Link[0]: Link { value: 7, next: None }
Key[0]: Key { key: 12 78 }
"#
        );

        // Example 2: new key is a prefix of the old key
        let mut index = open_opts().open(dir.path().join("2")).expect("open");
        index.insert(&[0x12, 0x34], 5).expect("insert");
        index.flush().expect("flush");
        index.insert(&[0x12], 7).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 46, root: Radix[0] }
Disk[1]: Key { key: 12 34 }
Disk[5]: Link { value: 5, next: None }
Disk[8]: Leaf { key: Disk[1], link: Disk[5] }
Disk[11]: Radix { link: None, 1: Disk[8] }
Disk[19]: Root { radix: Disk[11] }
Disk[22]: Checksum { start: 0, end: 22, chunk_size_logarithm: 4, checksums.len(): 2 }
Radix[0]: Radix { link: None, 1: Radix[1] }
Radix[1]: Radix { link: None, 2: Radix[2] }
Radix[2]: Radix { link: Link[0], 3: Disk[8] }
Link[0]: Link { value: 7, next: None }
"#
        );

        // Example 3: old key is a prefix of the new key
        // Only one flush - only one key is written.
        let mut index = open_opts().open(dir.path().join("3a")).expect("open");
        index.insert(&[0x12], 5).expect("insert");
        index.insert(&[0x12, 0x78], 7).expect("insert");
        index.flush().expect("flush");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 77, root: Disk[34] }
Disk[1]: Key { key: 12 78 }
Disk[5]: Link { value: 5, next: None }
Disk[8]: Link { value: 7, next: None }
Disk[11]: Leaf { key: Disk[1], link: Disk[8] }
Disk[14]: Radix { link: Disk[5], 7: Disk[11] }
Disk[26]: Radix { link: None, 2: Disk[14] }
Disk[34]: Radix { link: None, 1: Disk[26] }
Disk[42]: Root { radix: Disk[34] }
Disk[45]: Checksum { start: 0, end: 45, chunk_size_logarithm: 4, checksums.len(): 3 }
"#
        );

        // With two flushes - the old key cannot be removed since it was written.
        let mut index = open_opts().open(dir.path().join("3b")).expect("open");
        index.insert(&[0x12], 5).expect("insert");
        index.flush().expect("flush");
        index.insert(&[0x12, 0x78], 7).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 45, root: Radix[0] }
Disk[1]: Key { key: 12 }
Disk[4]: Link { value: 5, next: None }
Disk[7]: Leaf { key: Disk[1], link: Disk[4] }
Disk[10]: Radix { link: None, 1: Disk[7] }
Disk[18]: Root { radix: Disk[10] }
Disk[21]: Checksum { start: 0, end: 21, chunk_size_logarithm: 4, checksums.len(): 2 }
Radix[0]: Radix { link: None, 1: Radix[1] }
Radix[1]: Radix { link: None, 2: Radix[2] }
Radix[2]: Radix { link: Disk[4], 7: Leaf[0] }
Leaf[0]: Leaf { key: Key[0], link: Link[0] }
Link[0]: Link { value: 7, next: None }
Key[0]: Key { key: 12 78 }
"#
        );

        // Same key. Multiple values.
        let mut index = open_opts().open(dir.path().join("4")).expect("open");
        index.insert(&[0x12], 5).expect("insert");
        index.flush().expect("flush");
        index.insert(&[0x12], 7).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 45, root: Radix[0] }
Disk[1]: Key { key: 12 }
Disk[4]: Link { value: 5, next: None }
Disk[7]: Leaf { key: Disk[1], link: Disk[4] }
Disk[10]: Radix { link: None, 1: Disk[7] }
Disk[18]: Root { radix: Disk[10] }
Disk[21]: Checksum { start: 0, end: 21, chunk_size_logarithm: 4, checksums.len(): 2 }
Radix[0]: Radix { link: None, 1: Leaf[0] }
Leaf[0]: Leaf { key: Disk[1], link: Link[0] }
Link[0]: Link { value: 7, next: Disk[4] }
"#
        );
    }

    #[test]
    fn test_external_keys() {
        let buf = Arc::new(vec![0x12u8, 0x34, 0x56, 0x78, 0x9a, 0xbc]);
        let dir = tempdir().unwrap();
        let mut index = open_opts()
            .key_buf(Some(buf))
            .open(dir.path().join("a"))
            .expect("open");
        index
            .insert_advanced(InsertKey::Reference((1, 2)), InsertValue::Prepend(55))
            .expect("insert");
        index.flush().expect("flush");
        index
            .insert_advanced(InsertKey::Reference((1, 3)), InsertValue::Prepend(77))
            .expect("insert");
        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 43, root: Radix[0] }
Disk[1]: InlineLeaf { key: Disk[2], link: Disk[5] }
Disk[2]: ExtKey { start: 1, len: 2 }
Disk[5]: Link { value: 55, next: None }
Disk[8]: Radix { link: None, 3: Disk[1] }
Disk[16]: Root { radix: Disk[8] }
Disk[19]: Checksum { start: 0, end: 19, chunk_size_logarithm: 4, checksums.len(): 2 }
Radix[0]: Radix { link: None, 3: Radix[1] }
Radix[1]: Radix { link: None, 4: Radix[2] }
Radix[2]: Radix { link: None, 5: Radix[3] }
Radix[3]: Radix { link: None, 6: Radix[4] }
Radix[4]: Radix { link: Disk[5], 7: Leaf[0] }
Leaf[0]: Leaf { key: ExtKey[0], link: Link[0] }
Link[0]: Link { value: 77, next: None }
ExtKey[0]: ExtKey { start: 1, len: 3 }
"#
        );
    }

    #[test]
    fn test_inline_leafs() {
        let buf = Arc::new(vec![0x12u8, 0x34, 0x56, 0x78, 0x9a, 0xbc]);
        let dir = tempdir().unwrap();
        let mut index = open_opts()
            .key_buf(Some(buf))
            .open(dir.path().join("a"))
            .expect("open");

        // New entry. Should be inlined.
        index
            .insert_advanced(InsertKey::Reference((1, 1)), InsertValue::Prepend(55))
            .unwrap();
        index.flush().expect("flush");

        // Independent leaf. Should also be inlined.
        index
            .insert_advanced(InsertKey::Reference((2, 1)), InsertValue::Prepend(77))
            .unwrap();
        index.flush().expect("flush");

        // The link with 88 should refer to the inlined leaf 77.
        index
            .insert_advanced(InsertKey::Reference((2, 1)), InsertValue::Prepend(88))
            .unwrap();
        index.flush().expect("flush");

        // Not inlined because dependent link was not written first.
        // (could be optimized in the future)
        index
            .insert_advanced(InsertKey::Reference((3, 1)), InsertValue::Prepend(99))
            .unwrap();
        index
            .insert_advanced(InsertKey::Reference((3, 1)), InsertValue::Prepend(100))
            .unwrap();
        index.flush().expect("flush");

        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 257, root: Disk[181] }
Disk[1]: InlineLeaf { key: Disk[2], link: Disk[5] }
Disk[2]: ExtKey { start: 1, len: 1 }
Disk[5]: Link { value: 55, next: None }
Disk[8]: Radix { link: None, 3: Disk[1] }
Disk[16]: Root { radix: Disk[8] }
Disk[19]: Checksum { start: 0, end: 19, chunk_size_logarithm: 4, checksums.len(): 2 }
Disk[43]: InlineLeaf { key: Disk[44], link: Disk[47] }
Disk[44]: ExtKey { start: 2, len: 1 }
Disk[47]: Link { value: 77, next: None }
Disk[50]: Radix { link: None, 3: Disk[1], 5: Disk[43] }
Disk[62]: Root { radix: Disk[50] }
Disk[65]: Checksum { start: 19, end: 65, chunk_size_logarithm: 4, checksums.len(): 4 }
Disk[105]: Link { value: 88, next: Disk[47] }
Disk[108]: Leaf { key: Disk[44], link: Disk[105] }
Disk[111]: Radix { link: None, 3: Disk[1], 5: Disk[108] }
Disk[123]: Root { radix: Disk[111] }
Disk[126]: Checksum { start: 65, end: 126, chunk_size_logarithm: 4, checksums.len(): 4 }
Disk[166]: ExtKey { start: 3, len: 1 }
Disk[169]: Link { value: 99, next: None }
Disk[172]: Link { value: 100, next: Disk[169] }
Disk[176]: Leaf { key: Disk[166], link: Disk[172] }
Disk[181]: Radix { link: None, 3: Disk[1], 5: Disk[108], 7: Disk[176] }
Disk[197]: Root { radix: Disk[181] }
Disk[201]: Checksum { start: 126, end: 201, chunk_size_logarithm: 4, checksums.len(): 6 }
"#
        )
    }

    #[test]
    fn test_clone() {
        let dir = tempdir().unwrap();
        let mut index = open_opts().open(dir.path().join("a")).expect("open");

        // Test clone empty index
        assert_eq!(
            format!("{:?}", index.try_clone().unwrap()),
            format!("{:?}", index)
        );
        assert_eq!(
            format!("{:?}", index.try_clone_without_dirty().unwrap()),
            format!("{:?}", index)
        );

        // Test on-disk Index
        index.insert(&[], 55).expect("insert");
        index.insert(&[0x12], 77).expect("insert");
        index.flush().expect("flush");
        index.insert(&[0x15], 99).expect("insert");

        let mut index2 = index.try_clone().expect("clone");
        assert_eq!(format!("{:?}", index), format!("{:?}", index2));

        // Test clone without in-memory part
        let index2clean = index.try_clone_without_dirty().unwrap();
        index2.clear_dirty();
        assert_eq!(format!("{:?}", index2), format!("{:?}", index2clean));

        // Test in-memory Index
        let mut index3 = open_opts()
            .checksum_chunk_size_logarithm(0)
            .create_in_memory()
            .unwrap();
        let index4 = index3.try_clone().unwrap();
        assert_eq!(format!("{:?}", index3), format!("{:?}", index4));

        index3.insert(&[0x15], 99).expect("insert");
        let index4 = index3.try_clone().unwrap();
        assert_eq!(format!("{:?}", index3), format!("{:?}", index4));
    }

    #[test]
    fn test_open_options_write() {
        let dir = tempdir().unwrap();
        let mut index = OpenOptions::new().open(dir.path().join("a")).expect("open");
        index.insert(&[0x12], 77).expect("insert");
        index.flush().expect("flush");

        OpenOptions::new()
            .write(Some(false))
            .open(dir.path().join("b"))
            .expect_err("open"); // file does not exist

        let mut index = OpenOptions::new()
            .write(Some(false))
            .open(dir.path().join("a"))
            .expect("open");
        index.flush().expect_err("cannot flush read-only index");
    }

    #[test]
    fn test_linked_list_values() {
        let dir = tempdir().unwrap();
        let mut index = OpenOptions::new().open(dir.path().join("a")).expect("open");
        let list = vec![11u64, 17, 19, 31];
        for i in list.iter().rev() {
            index.insert(&[], *i).expect("insert");
        }

        let list1: Vec<u64> = index
            .get(&[])
            .unwrap()
            .values(&index)
            .map(|v| v.unwrap())
            .collect();
        assert_eq!(list, list1);

        index.flush().expect("flush");
        let list2: Vec<u64> = index
            .get(&[])
            .unwrap()
            .values(&index)
            .map(|v| v.unwrap())
            .collect();
        assert_eq!(list, list2);

        // Empty linked list
        assert_eq!(index.get(&[1]).unwrap().values(&index).count(), 0);

        // In case error happens, the iteration still stops.
        index.insert(&[], 5).expect("insert");
        index.dirty_links[0].next_link_offset = LinkOffset(Offset(1000));
        // Note: `collect` can return `crate::Result<Vec<u64>>`. But that does not exercises the
        // infinite loop avoidance logic since `collect` stops iteration at the first error.
        let list_errored: Vec<crate::Result<u64>> =
            index.get(&[]).unwrap().values(&index).collect();
        assert!(list_errored[list_errored.len() - 1].is_err());
    }

    #[test]
    fn test_checksum_bitflip_1b() {
        test_checksum_bitflip_with_size(0);
    }

    #[test]
    fn test_checksum_bitflip_2b() {
        test_checksum_bitflip_with_size(1);
    }

    #[test]
    fn test_checksum_bitflip_16b() {
        test_checksum_bitflip_with_size(4);
    }

    #[test]
    fn test_checksum_bitflip_1mb() {
        test_checksum_bitflip_with_size(20);
    }

    fn test_checksum_bitflip_with_size(checksum_log_size: u32) {
        let dir = tempdir().unwrap();

        let keys = if cfg!(debug_assertions) {
            // Debug build is much slower than release build. Limit the key length to 1-byte.
            vec![vec![0x13], vec![0x17], vec![]]
        } else {
            // Release build can afford 2-byte key test.
            vec![
                vec![0x12, 0x34],
                vec![0x12, 0x78],
                vec![0x34, 0x56],
                vec![0x34],
                vec![0x78],
                vec![0x78, 0x9a],
            ]
        };

        let opts = open_opts()
            .checksum_chunk_size_logarithm(checksum_log_size)
            .clone();

        let bytes = {
            let mut index = opts.open(dir.path().join("a")).expect("open");

            for (i, key) in keys.iter().enumerate() {
                index.insert(key, i as u64).expect("insert");
                index.insert(key, (i as u64) << 50).expect("insert");
            }
            index.flush().expect("flush");

            // Read the raw bytes of the index content
            let mut f = File::open(dir.path().join("a")).expect("open");
            let mut buf = vec![];
            f.read_to_end(&mut buf).expect("read");

            // Drop `index` here. This would unmap files so File::create below
            // can work on Windows.
            if std::env::var("DEBUG").is_ok() {
                eprintln!("{:?}", &index);
            }

            buf
        };

        fn is_corrupted(index: &Index, key: &[u8]) -> bool {
            let link = index.get(&key);
            match link {
                Err(_) => true,
                Ok(link) => link.values(index).any(|v| v.is_err()),
            }
        }

        // Every bit change should trigger errors when reading all contents
        for i in 0..(bytes.len() * 8) {
            let mut bytes = bytes.clone();
            bytes[i / 8] ^= 1u8 << (i % 8);
            let mut f = File::create(dir.path().join("a")).expect("create");
            f.write_all(&bytes).expect("write");

            let index = opts.clone().open(dir.path().join("a"));
            let detected = match index {
                Err(_) => true,
                Ok(index) => {
                    let range = if cfg!(debug_assertions) { 0 } else { 0x10000 };
                    (0..range).any(|key_int| {
                        let key = [(key_int >> 8) as u8, (key_int & 0xff) as u8];
                        is_corrupted(&index, &key)
                    }) || (0..0x100).any(|key_int| {
                        let key = [key_int as u8];
                        is_corrupted(&index, &key)
                    }) || is_corrupted(&index, &[])
                }
            };
            assert!(
                detected,
                "bit flip at byte {} , bit {} is not detected (set DEBUG=1 to see Index dump)",
                i / 8,
                i % 8,
            );
        }
    }

    #[test]
    fn test_checksum_toggle() {
        let dir = tempdir().unwrap();
        let open = |enabled: bool| {
            open_opts()
                .checksum_enabled(enabled)
                .open(dir.path().join("a"))
                .expect("open")
        };

        // Starting with checksum off.
        let mut index = open(false);
        index.verify().unwrap();
        index.insert(b"abcdefg", 0x1234).unwrap();
        index.flush().unwrap();
        index.insert(b"bcdefgh", 0x2345).unwrap();
        index.flush().unwrap();

        // Turn on.
        let mut index = open(true);
        index.verify().unwrap();
        index.insert(b"cdefghi", 0x3456).unwrap();
        index.flush().unwrap();
        index.insert(b"defghij", 0x4567).unwrap();
        index.flush().unwrap();

        // Turn off.
        let mut index = open(false);
        index.verify().unwrap();
        index.insert(b"efghijh", 0x5678).unwrap();
        index.flush().unwrap();
        index.insert(b"fghijkl", 0x7890).unwrap();
        index.flush().unwrap();

        assert_eq!(
            format!("{:?}", index),
            r#"Index { len: 415, root: Disk[402] }
Disk[1]: Key { key: 61 62 63 64 65 66 67 }
Disk[10]: Link { value: 4660, next: None }
Disk[14]: Leaf { key: Disk[1], link: Disk[10] }
Disk[17]: Radix { link: None, 6: Disk[14] }
Disk[25]: Root { radix: Disk[17] }
Disk[29]: Key { key: 62 63 64 65 66 67 68 }
Disk[38]: Link { value: 9029, next: None }
Disk[42]: Leaf { key: Disk[29], link: Disk[38] }
Disk[45]: Radix { link: None, 1: Disk[14], 2: Disk[42] }
Disk[57]: Radix { link: None, 6: Disk[45] }
Disk[65]: Root { radix: Disk[57] }
Disk[69]: Key { key: 63 64 65 66 67 68 69 }
Disk[78]: Link { value: 13398, next: None }
Disk[82]: Leaf { key: Disk[69], link: Disk[78] }
Disk[85]: Radix { link: None, 1: Disk[14], 2: Disk[42], 3: Disk[82] }
Disk[101]: Radix { link: None, 6: Disk[85] }
Disk[109]: Root { radix: Disk[101] }
Disk[112]: Checksum { start: 0, end: 112, chunk_size_logarithm: 4, checksums.len(): 7 }
Disk[176]: Key { key: 64 65 66 67 68 69 6A }
Disk[185]: Link { value: 17767, next: None }
Disk[190]: Leaf { key: Disk[176], link: Disk[185] }
Disk[195]: Radix { link: None, 1: Disk[14], 2: Disk[42], 3: Disk[82], 4: Disk[190] }
Disk[215]: Radix { link: None, 6: Disk[195] }
Disk[223]: Root { radix: Disk[215] }
Disk[227]: Checksum { start: 112, end: 227, chunk_size_logarithm: 4, checksums.len(): 8 }
Disk[299]: Key { key: 65 66 67 68 69 6A 68 }
Disk[308]: Link { value: 22136, next: None }
Disk[313]: Leaf { key: Disk[299], link: Disk[308] }
Disk[318]: Radix { link: None, 1: Disk[14], 2: Disk[42], 3: Disk[82], 4: Disk[190], 5: Disk[313] }
Disk[342]: Radix { link: None, 6: Disk[318] }
Disk[350]: Root { radix: Disk[342] }
Disk[355]: Key { key: 66 67 68 69 6A 6B 6C }
Disk[364]: Link { value: 30864, next: None }
Disk[369]: Leaf { key: Disk[355], link: Disk[364] }
Disk[374]: Radix { link: None, 1: Disk[14], 2: Disk[42], 3: Disk[82], 4: Disk[190], 5: Disk[313], 6: Disk[369] }
Disk[402]: Radix { link: None, 6: Disk[374] }
Disk[410]: Root { radix: Disk[402] }
"#
        );
    }

    fn show_checksums(index: &Index) -> String {
        let debug_str = format!("{:?}", index);
        debug_str
            .lines()
            .filter_map(|l| {
                if l.contains("Checksum") {
                    Some(format!("\n                {}", l))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .concat()
    }

    #[test]
    fn test_checksum_max_chain_len() {
        // Test with the given max_chain_len config.
        let t = |max_chain_len: u32| {
            let dir = tempdir().unwrap();
            let path = dir.path().join("i");
            let mut index = open_opts()
                .checksum_chunk_size_logarithm(7 /* chunk size: 127 */)
                .checksum_max_chain_len(max_chain_len)
                .open(&path)
                .unwrap();
            for i in 0..10u8 {
                let data: Vec<u8> = if i % 2 == 0 { vec![i] } else { vec![i; 100] };
                index.insert(&data, 1).unwrap();
                index.flush().unwrap();
                index.verify().unwrap();
                // If reload from disk, it pass verification too.
                let mut index2 = open_opts()
                    .checksum_max_chain_len(max_chain_len)
                    .open(&path)
                    .unwrap();
                index2.verify().unwrap();
                // Create "racy" writes by flushing from another index.
                if i % 3 == 0 {
                    index2.insert(&data, 2).unwrap();
                    index2.flush().unwrap();
                }
            }
            show_checksums(&index)
        };

        // Unlimited chain. Chain: 1358 -> 1167 -> 1071 -> 818 -> 738 -> ...
        assert_eq!(
            t(0),
            r#"
                Disk[21]: Checksum { start: 0, end: 21, chunk_size_logarithm: 7, checksums.len(): 1 }
                Disk[54]: Checksum { start: 0, end: 54, chunk_size_logarithm: 7, checksums.len(): 1 }
                Disk[203]: Checksum { start: 0, end: 203, chunk_size_logarithm: 7, checksums.len(): 2 }
                Disk[266]: Checksum { start: 203, end: 266, chunk_size_logarithm: 7, checksums.len(): 2 }
                Disk[433]: Checksum { start: 266, end: 433, chunk_size_logarithm: 7, checksums.len(): 2 }
                Disk[499]: Checksum { start: 433, end: 499, chunk_size_logarithm: 7, checksums.len(): 1 }
                Disk[563]: Checksum { start: 433, end: 563, chunk_size_logarithm: 7, checksums.len(): 2 }
                Disk[738]: Checksum { start: 563, end: 738, chunk_size_logarithm: 7, checksums.len(): 2 }
                Disk[818]: Checksum { start: 738, end: 818, chunk_size_logarithm: 7, checksums.len(): 2 }
                Disk[896]: Checksum { start: 818, end: 896, chunk_size_logarithm: 7, checksums.len(): 1 }
                Disk[1071]: Checksum { start: 818, end: 1071, chunk_size_logarithm: 7, checksums.len(): 3 }
                Disk[1167]: Checksum { start: 1071, end: 1167, chunk_size_logarithm: 7, checksums.len(): 2 }
                Disk[1358]: Checksum { start: 1167, end: 1358, chunk_size_logarithm: 7, checksums.len(): 2 }"#
        );

        // Max chain len = 2. Chain: 1331 -> 1180 -> 0; 872 -> 761 -> 0; ...
        assert_eq!(
            t(2),
            r#"
                Disk[21]: Checksum { start: 0, end: 21, chunk_size_logarithm: 7, checksums.len(): 1 }
                Disk[54]: Checksum { start: 0, end: 54, chunk_size_logarithm: 7, checksums.len(): 1 }
                Disk[203]: Checksum { start: 0, end: 203, chunk_size_logarithm: 7, checksums.len(): 2 }
                Disk[266]: Checksum { start: 203, end: 266, chunk_size_logarithm: 7, checksums.len(): 2 }
                Disk[433]: Checksum { start: 0, end: 433, chunk_size_logarithm: 7, checksums.len(): 4 }
                Disk[514]: Checksum { start: 433, end: 514, chunk_size_logarithm: 7, checksums.len(): 2 }
                Disk[586]: Checksum { start: 433, end: 586, chunk_size_logarithm: 7, checksums.len(): 2 }
                Disk[761]: Checksum { start: 0, end: 761, chunk_size_logarithm: 7, checksums.len(): 6 }
                Disk[872]: Checksum { start: 761, end: 872, chunk_size_logarithm: 7, checksums.len(): 2 }
                Disk[950]: Checksum { start: 0, end: 950, chunk_size_logarithm: 7, checksums.len(): 8 }
                Disk[1180]: Checksum { start: 0, end: 1180, chunk_size_logarithm: 7, checksums.len(): 10 }
                Disk[1331]: Checksum { start: 1180, end: 1331, chunk_size_logarithm: 7, checksums.len(): 2 }
                Disk[1522]: Checksum { start: 0, end: 1522, chunk_size_logarithm: 7, checksums.len(): 12 }"#
        );

        // Max chain len = 1. All have start: 0.
        assert_eq!(
            t(1),
            r#"
                Disk[21]: Checksum { start: 0, end: 21, chunk_size_logarithm: 7, checksums.len(): 1 }
                Disk[54]: Checksum { start: 0, end: 54, chunk_size_logarithm: 7, checksums.len(): 1 }
                Disk[203]: Checksum { start: 0, end: 203, chunk_size_logarithm: 7, checksums.len(): 2 }
                Disk[266]: Checksum { start: 0, end: 266, chunk_size_logarithm: 7, checksums.len(): 3 }
                Disk[440]: Checksum { start: 0, end: 440, chunk_size_logarithm: 7, checksums.len(): 4 }
                Disk[521]: Checksum { start: 0, end: 521, chunk_size_logarithm: 7, checksums.len(): 5 }
                Disk[616]: Checksum { start: 0, end: 616, chunk_size_logarithm: 7, checksums.len(): 5 }
                Disk[814]: Checksum { start: 0, end: 814, chunk_size_logarithm: 7, checksums.len(): 7 }
                Disk[933]: Checksum { start: 0, end: 933, chunk_size_logarithm: 7, checksums.len(): 8 }
                Disk[1058]: Checksum { start: 0, end: 1058, chunk_size_logarithm: 7, checksums.len(): 9 }
                Disk[1296]: Checksum { start: 0, end: 1296, chunk_size_logarithm: 7, checksums.len(): 11 }
                Disk[1455]: Checksum { start: 0, end: 1455, chunk_size_logarithm: 7, checksums.len(): 12 }
                Disk[1725]: Checksum { start: 0, end: 1725, chunk_size_logarithm: 7, checksums.len(): 14 }"#
        );
    }

    #[test]
    fn test_root_meta() {
        let dir = tempdir().unwrap();
        let mut index = open_opts().open(dir.path().join("a")).expect("open");
        assert!(index.get_meta().is_empty());
        let meta = vec![200; 4000];
        index.set_meta(&meta);
        assert_eq!(index.get_meta(), &meta[..]);
        index.flush().expect("flush");
        let index = open_opts().open(dir.path().join("a")).expect("open");
        assert_eq!(index.get_meta(), &meta[..]);
    }

    impl<'a> RangeIter<'a> {
        fn clone_with_index(&self, index: &'a Index) -> Self {
            Self {
                completed: self.completed,
                index,
                front_stack: self.front_stack.clone(),
                back_stack: self.back_stack.clone(),
            }
        }
    }

    /// Extract keys from the [`RangeIter`]. Verify different iteration
    /// directions, and returned link offsets. A `key` has link offset `i`
    /// if that key matches `keys[i]`.
    fn iter_to_keys(index: &Index, keys: &[&[u8]], iter: &RangeIter) -> Vec<Vec<u8>> {
        let it_forward = iter.clone_with_index(index);
        let it_backward = iter.clone_with_index(index);
        let mut it_both_ends = iter.clone_with_index(index);

        let extract = |v: crate::Result<(Cow<'_, [u8]>, LinkOffset)>| -> Vec<u8> {
            let (key, link_offset) = v.unwrap();
            let key = key.as_ref();
            // Verify link_offset is correct
            let ids: Vec<u64> = link_offset
                .values(index)
                .collect::<crate::Result<Vec<u64>>>()
                .unwrap();
            assert!(ids.len() == 1);
            assert_eq!(keys[ids[0] as usize], key);
            key.to_vec()
        };

        let keys_forward: Vec<_> = it_forward.map(extract).collect();
        let mut keys_backward: Vec<_> = it_backward.rev().map(extract).collect();
        keys_backward.reverse();
        assert_eq!(keys_forward, keys_backward);

        // Forward and backward iterators should not overlap
        let mut keys_both_ends = Vec::new();
        for i in 0..(keys_forward.len() + 2) {
            if let Some(v) = it_both_ends.next() {
                keys_both_ends.insert(i, extract(v));
            }
            if let Some(v) = it_both_ends.next_back() {
                keys_both_ends.insert(i + 1, extract(v));
            }
        }
        assert_eq!(keys_forward, keys_both_ends);

        keys_forward
    }

    /// Test `Index::range` against `BTreeSet::range`. `tree` specifies keys.
    fn test_range_against_btreeset(tree: BTreeSet<&[u8]>) {
        let dir = tempdir().unwrap();
        let mut index = open_opts().open(dir.path().join("a")).unwrap();
        let keys: Vec<&[u8]> = tree.iter().cloned().collect();
        for (i, key) in keys.iter().enumerate() {
            index.insert(key, i as u64).unwrap();
        }

        let range_test = |start: Bound<&[u8]>, end: Bound<&[u8]>| {
            let range = (start, end);
            let iter = index.range(range).unwrap();
            let expected_keys: Vec<Vec<u8>> = tree
                .range::<&[u8], _>((start, end))
                .map(|v| v.to_vec())
                .collect();
            let selected_keys: Vec<Vec<u8>> = iter_to_keys(&index, &keys, &iter);
            assert_eq!(selected_keys, expected_keys);
        };

        // Generate key variants based on existing keys. Generated keys do not
        // exist int the index. Therefore the test is more interesting.
        let mut variant_keys = Vec::new();
        for base_key in keys.iter() {
            // One byte appended
            for b in [0x00, 0x77, 0xff] {
                let mut key = base_key.to_vec();
                key.push(b);
                variant_keys.push(key);
            }

            // Last byte mutated, or removed
            if !base_key.is_empty() {
                let mut key = base_key.to_vec();
                let last = *key.last().unwrap();
                *key.last_mut().unwrap() = last.wrapping_add(1);
                variant_keys.push(key.clone());
                *key.last_mut().unwrap() = last.wrapping_sub(1);
                variant_keys.push(key.clone());
                key.pop();
                variant_keys.push(key);
            }
        }

        // Remove duplicated entries.
        let variant_keys = variant_keys
            .iter()
            .map(|v| v.as_ref())
            .filter(|k| !tree.contains(k))
            .collect::<BTreeSet<&[u8]>>()
            .iter()
            .cloned()
            .collect::<Vec<&[u8]>>();

        range_test(Unbounded, Unbounded);

        for key1 in keys.iter().chain(variant_keys.iter()) {
            range_test(Unbounded, Included(key1));
            range_test(Unbounded, Excluded(key1));
            range_test(Included(key1), Unbounded);
            range_test(Excluded(key1), Unbounded);

            for key2 in keys.iter().chain(variant_keys.iter()) {
                if key1 < key2 {
                    range_test(Excluded(key1), Excluded(key2));
                }
                if key1 <= key2 {
                    range_test(Excluded(key1), Included(key2));
                    range_test(Included(key1), Excluded(key2));
                    range_test(Included(key1), Included(key2));
                }
            }
        }
    }

    #[test]
    fn test_range_example1() {
        test_range_against_btreeset(
            [
                &[0x00, 0x00, 0x00][..],
                &[0x10, 0x0d, 0x01],
                &[0x10, 0x0e],
                &[0x10, 0x0f],
                &[0x10, 0x0f, 0xff],
                &[0x10, 0x10, 0x01],
                &[0x10, 0x11],
                &[0xff],
            ]
            .iter()
            .cloned()
            .collect(),
        );
    }

    #[test]
    fn test_clear_dirty() {
        let dir = tempdir().unwrap();
        let mut index = open_opts().open(dir.path().join("a")).unwrap();

        index.insert(&"foo", 2).unwrap();
        assert!(!index.get(&"foo").unwrap().is_null());

        index.clear_dirty();
        assert!(index.get(&"foo").unwrap().is_null());

        index.set_meta(vec![42]);
        index.insert(&"foo", 1).unwrap();
        index.flush().unwrap();

        index.set_meta(vec![43]);
        index.insert(&"bar", 2).unwrap();

        assert_eq!(index.get_original_meta(), [42]);
        index.clear_dirty();

        assert_eq!(index.get_meta(), [42]);
        assert!(index.get(&"bar").unwrap().is_null());
    }

    #[test]
    fn test_meta_only_flush() {
        let dir = tempdir().unwrap();
        let mut index = open_opts().open(dir.path().join("a")).unwrap();

        index.set_meta(b"foo");
        index.flush().unwrap();

        let mut index = open_opts().open(dir.path().join("a")).unwrap();
        assert_eq!(index.get_meta(), b"foo");

        index.set_meta(b"bar");
        let len1 = index.flush().unwrap();

        let mut index = open_opts().open(dir.path().join("a")).unwrap();
        assert_eq!(index.get_meta(), b"bar");
        index.set_meta(b"bar");
        let len2 = index.flush().unwrap();
        assert_eq!(len1, len2);
    }

    #[test]
    fn test_flush_with_replaced_file() {
        let dir = tempdir().unwrap();
        let path1 = dir.path().join("1");
        let path2 = dir.path().join("2");
        let mut index1 = open_opts().open(&path1).unwrap();
        let mut index2 = open_opts().open(&path2).unwrap();

        index1.insert(b"foo1", 1).unwrap();
        index1.insert(b"z1", 1).unwrap();
        index2.insert(b"foo1", 2).unwrap();
        index2.insert(b"z1", 2).unwrap();

        let len1 = index1.flush().unwrap();
        let len2 = index2.flush().unwrap();

        assert_eq!(len1, len2);

        // Modify index1 so it has pending in-memory changes.
        index1.insert(b"z1", 3).unwrap();

        // Attempt to modify index1 by replacing its underlying file. This won't actaully break
        // index1, because Index operates at the file descriptor level, not the path level (unlike
        // Log), Index will not reload the file from the same path on flush.
        fs::rename(&path1, &path1.with_extension("bak")).unwrap();
        fs::rename(&path2, &path1).unwrap();

        // "index1.flush" failure shouldn't change its internal state.
        index1.fail_on_flush = 8;
        index1.flush().unwrap_err();
        let link_offset = index1.get(b"foo1").expect("lookup");
        // "foo1" in index1 should still be "1", not "2" in index2.
        assert_eq!(link_offset.value_and_next(&index1).unwrap().0, 1);
    }

    quickcheck! {
        fn test_single_value(map: HashMap<Vec<u8>, u64>, flush: bool) -> bool {
            let dir = tempdir().unwrap();
            let mut index = open_opts().open(dir.path().join("a")).expect("open");

            for (key, value) in &map {
                index.insert(key, *value).expect("insert");
            }

            if flush {
                let len = index.flush().expect("flush");
                index = open_opts().logical_len(len.into()).open(dir.path().join("a")).unwrap();
            }

            index.verify_against_hashmap(&map)
        }

        fn test_flush_failure(map: HashMap<Vec<u8>, u64>) -> bool {
            let dir = tempdir().unwrap();
            let mut index = open_opts().open(dir.path().join("a")).expect("open");

            // Populate `index` - some entries on disk, some in memory.
            for (key, value) in &map {
                if value % 2 == 0 {
                    index.insert(key, *value).expect("insert");
                }
            }
            index.flush().expect("flush");

            // flush should only fail if it is not a no-op - has pending entries in memory.
            let mut should_fail = false;
            for (key, value) in &map {
                if value % 2 == 1 {
                    index.insert(key, *value).expect("insert");
                    should_fail = true;
                }
            }

            (1..=8).all(|flush_failure| {
                let mut index = index.try_clone().expect("clone");
                index.fail_on_flush = flush_failure;
                let old_index_buf_len = index.buf.len();
                assert_eq!(index.flush().is_err(), should_fail);
                assert_eq!(old_index_buf_len, index.buf.len());

                // Verify entries in map.
                let mut verified = index.verify_against_hashmap(&map);

                // Try flush again - should succeed.
                if !should_fail {
                    index.fail_on_flush = 0;
                    index.flush().expect("flush should succeed");
                    verified &= index.verify_against_hashmap(&map)
                }

                verified
            })
        }

        fn test_multiple_values(map: HashMap<Vec<u8>, Vec<u64>>) -> bool {
            let dir = tempdir().unwrap();
            let mut index = open_opts().open(dir.path().join("a")).expect("open");
            let mut index_mem = open_opts().checksum_chunk_size_logarithm(20).create_in_memory().unwrap();

            for (key, values) in &map {
                for value in values.iter().rev() {
                    index.insert(key, *value).expect("insert");
                    index_mem.insert(key, *value).expect("insert");
                }
                if values.is_empty() {
                    // Flush sometimes.
                    index.flush().expect("flush");
                }
            }

            map.iter().all(|(key, values)| {
                let v: Vec<u64> =
                    index.get(key).unwrap().values(&index).map(|v| v.unwrap()).collect();
                let v_mem: Vec<u64> =
                    index_mem.get(key).unwrap().values(&index_mem).map(|v| v.unwrap()).collect();
                v == *values && v_mem == *values
            })
        }

        fn test_range_quickcheck(keys: Vec<Vec<u8>>) -> bool {
            let size_limit = if cfg!(debug_assertions) {
                4
            } else {
                16
            };
            let size = keys.len() % size_limit + 1;
            let tree: BTreeSet<&[u8]> = keys.iter().take(size).map(|v| v.as_ref()).collect();
            test_range_against_btreeset(tree);
            true
        }

        fn test_deletion(keys_deleted: Vec<(Vec<u8>, bool)>) -> bool {
            // Compare Index with BTreeSet
            let mut set = BTreeSet::<Vec<u8>>::new();
            let mut index = in_memory_index();
            keys_deleted.into_iter().all(|(key, deleted)| {
                if deleted {
                    set.remove(&key);
                    index.remove(&key).unwrap();
                } else {
                    set.insert(key.clone());
                    index.insert(&key, 1).unwrap();
                }
                index
                    .range(..)
                    .unwrap()
                    .map(|s| s.unwrap().0.as_ref().to_vec())
                    .collect::<Vec<_>>()
                    == set.iter().cloned().collect::<Vec<_>>()
            })
        }

        fn test_deletion_prefix(keys_deleted: Vec<(Vec<u8>, bool)>) -> bool {
            let mut set = BTreeSet::<Vec<u8>>::new();
            let mut index = in_memory_index();
            keys_deleted.into_iter().all(|(key, deleted)| {
                if deleted {
                    // BTreeSet does not have remove_prefix. Emulate it.
                    let to_delete = set
                        .iter()
                        .filter(|k| k.starts_with(&key))
                        .cloned()
                        .collect::<Vec<_>>();
                    for key in to_delete {
                        set.remove(&key);
                    }
                    index.remove_prefix(&key).unwrap();
                } else {
                    set.insert(key.clone());
                    index.insert(&key, 1).unwrap();
                }
                index
                    .range(..)
                    .unwrap()
                    .map(|s| s.unwrap().0.as_ref().to_vec())
                    .collect::<Vec<_>>()
                    == set.iter().cloned().collect::<Vec<_>>()
            })
        }
    }

    impl Index {
        fn verify_against_hashmap(&self, map: &HashMap<Vec<u8>, u64>) -> bool {
            let index = self;
            map.iter().all(|(key, value)| {
                let link_offset = index.get(key).expect("lookup");
                assert!(!link_offset.is_null());
                link_offset.value_and_next(&index).unwrap().0 == *value
            })
        }
    }
}

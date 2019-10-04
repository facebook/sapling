// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Index support for `log`.
//!
//! See [`Index`] for the main structure.

// File format:
//
// ```plain,ignore
// INDEX       := HEADER + ENTRY_LIST
// HEADER      := '\0'  (takes offset 0, so 0 is not a valid offset for ENTRY)
// ENTRY_LIST  := RADIX | ENTRY_LIST + ENTRY
// ENTRY       := RADIX | LEAF | LINK | KEY | ROOT + REVERSED(VLQ(ROOT_LEN))
// RADIX       := '\2' + RADIX_FLAG (1 byte) + BITMAP (2 bytes) +
//                PTR2(RADIX | LEAF) * popcnt(BITMAP) + PTR2(LINK)
// LEAF        := '\3' + PTR(KEY | EXT_KEY) + PTR(LINK)
// LINK        := '\4' + VLQ(VALUE) + PTR(NEXT_LINK | NULL)
// KEY         := '\5' + VLQ(KEY_LEN) + KEY_BYTES
// EXT_KEY     := '\6' + VLQ(KEY_START) + VLQ(KEY_LEN)
// INLINE_LEAF := '\7' + EXT_KEY + LINK
// ROOT        := '\1' + PTR(RADIX) + VLQ(META_LEN) + META
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
use std::cmp::Ordering::{Equal, Greater, Less};
use std::fmt::{self, Debug, Formatter};
use std::fs::{self, File};
use std::io::{self, Seek, SeekFrom, Write};
use std::mem::size_of;
use std::ops::{
    Bound::{self, Excluded, Included, Unbounded},
    Deref, RangeBounds,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::base16::{base16_to_base256, single_hex_to_base16, Base16Iter};
use crate::checksum_table::ChecksumTable;
use crate::errors::{IoResultExt, ResultExt};
use crate::lock::ScopedFileLock;
use crate::utils::{mmap_empty, mmap_readonly};

use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use fs2::FileExt;
use memmap::Mmap;
use vlqencoding::{VLQDecodeAt, VLQEncode};

//// Structures and serialization

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

// Shorter alias to `Option<ChecksumTable>`
type Checksum = Option<ChecksumTable>;

/// Read reversed vlq at the given end offset (exclusive).
/// Return the decoded integer and the bytes used by the VLQ integer.
fn read_vlq_reverse(buf: &[u8], end_offset: usize) -> io::Result<(u64, usize)> {
    let buf = buf.as_ref();
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

#[derive(Copy, Clone)]
enum TypedOffset {
    Radix(RadixOffset),
    Leaf(LeafOffset),
    Link(LinkOffset),
    Key(KeyOffset),
    ExtKey(ExtKeyOffset),
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
                Some(x) => Ok(*x as u8),
                _ => return Err(index.range_error(self.0 as usize, 1)),
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

    #[inline]
    fn type_int() -> u8;

    #[inline]
    fn from_offset_unchecked(offset: Offset) -> Self;

    #[inline]
    fn to_offset(&self) -> Offset;
}

impl_offset!(RadixOffset, TYPE_RADIX, "Radix");
impl_offset!(LeafOffset, TYPE_LEAF, "Leaf");
impl_offset!(LinkOffset, TYPE_LINK, "Link");
impl_offset!(KeyOffset, TYPE_KEY, "Key");
impl_offset!(ExtKeyOffset, TYPE_EXT_KEY, "ExtKey");

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
            let entry = MemRadix::read_from(&index, u64::from(self))?;
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
            index.dirty_radixes[self.dirty_index()].link_offset = value.into();
        } else {
            panic!("bug: set_link called on immutable radix entry");
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
            .map(|buf| LittleEndian::read_u16(buf))
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
            8 => buf
                .get(offset..offset + 8)
                .map(|buf| LittleEndian::read_u64(buf)),
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
        _ => Err(index
            .corruption(format!("unexpected key type at {}", key_offset.0))
            .into()),
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
                    index.verify_checksum(
                        u64::from(self),
                        raw_link_offset as u64 - u64::from(self),
                    )?;
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
                LinkOffset::from_offset(Offset::from_disk(index, raw_link_offset as u64)?, index)?;
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
                _ => (),
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
    fn key(stack: &Vec<IterState>, index: &Index) -> crate::Result<Vec<u8>> {
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
                        Ok(_) => Some(Err(index
                            .corruption("unexpected type during iteration")
                            .into())),
                        Err(err) => Some(Err(err.into())),
                    },
                    Err(err) => Some(Err(err)),
                },
                IterState::RadixLeaf(radix) => match radix.link_offset(index) {
                    Ok(link_offset) if link_offset.is_null() => continue,
                    Ok(link_offset) => match Self::key(stack, index) {
                        Ok(key) => Some(Ok((Cow::Owned(key), link_offset))),
                        Err(err) => Some(Err(err.into())),
                    },
                    Err(err) => Some(Err(err.into())),
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
            _ => (),
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
            _ => (),
        }
        result
    }
}

impl LinkOffset {
    /// Iterating through values referred by this linked list.
    pub fn values<'a>(self, index: &'a Index) -> LeafValueIter<'a> {
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
            next_link_offset: self.into(),
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
                Err(index.range_error(start, end - start).into())
            } else {
                Ok(&index.buf[start..end])
            }
        }
    }

    /// Create a new in-memory key entry. The key cannot be empty.
    #[inline]
    fn create(index: &mut Index, key: &[u8]) -> KeyOffset {
        debug_assert!(key.len() > 0);
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
        Ok((key_buf.slice(start, len), entry_size))
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
        Err(index.corruption(format!("type mismatch at offset {}", offset)))
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
        for i in 0..16 {
            if (bitmap >> i) & 1 == 1 {
                offsets[i] = Offset::from_disk(
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
        let u32_max = ::std::u32::MAX as u64;

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
        for i in 0..16 {
            let child_offset = self.offsets[i];
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
            _ => Err(index.range_error(offset, 1).into()),
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
        dirty_ext_keys: &mut Vec<MemExtKey>,
        dirty_links: &mut Vec<MemLink>,
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
    fn read_from(index: impl IndexBuf, offset: u64) -> crate::Result<Self> {
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
        Ok(MemRoot {
            radix_offset,
            meta: meta.to_vec().into_boxed_slice(),
        })
    }

    fn read_from_end(index: impl IndexBuf, end: u64) -> crate::Result<Self> {
        let buf = index.buf();
        if end > 1 {
            let (root_size, vlq_size) = read_vlq_reverse(buf, end as usize)
                .context(
                    index.path(),
                    "cannot read root_size in MemRoot::read_from_end",
                )
                .corruption()?;
            let vlq_size = vlq_size as u64;
            index.verify_checksum(end - vlq_size, vlq_size)?;
            Self::read_from(index, end - vlq_size - root_size)
        } else {
            Err(index
                .corruption(format!(
                    "index::MemRoot::read_from_end received an 'end' that is too small ({})",
                    end
                ))
                .into())
        }
    }

    fn write_to<W: Write>(&self, writer: &mut W, offset_map: &OffsetMap) -> io::Result<()> {
        let mut buf = Vec::with_capacity(16);
        buf.write_all(&[TYPE_ROOT])?;
        buf.write_vlq(self.radix_offset.to_disk(offset_map))?;
        buf.write_vlq(self.meta.len())?;
        buf.write_all(&self.meta)?;
        let len = buf.len();
        let mut reversed_vlq = Vec::new();
        reversed_vlq.write_vlq(len)?;
        reversed_vlq.reverse();
        buf.write_all(&reversed_vlq)?;
        writer.write_all(&buf)?;
        Ok(())
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
struct SimpleIndexBuf<'a>(&'a [u8], &'a Path, &'a Checksum);

impl<'a> IndexBuf for SimpleIndexBuf<'a> {
    fn buf(&self) -> &[u8] {
        self.0
    }
    fn checksum(&self) -> &Checksum {
        &self.2
    }
    fn path(&self) -> &Path {
        &self.1
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
            let dummy = SimpleIndexBuf(b"", Path::new("<dummy>"), &None);
            let result = match offset.to_typed(dummy).unwrap() {
                // Radix entries are pushed in the reversed order. So the index needs to be
                // reversed.
                TypedOffset::Radix(x) => self.radix_map[self.radix_len - 1 - x.dirty_index()],
                TypedOffset::Leaf(x) => self.leaf_map[x.dirty_index()],
                TypedOffset::Link(x) => self.link_map[x.dirty_index()],
                TypedOffset::Key(x) => self.key_map[x.dirty_index()],
                TypedOffset::ExtKey(x) => self.ext_key_map[x.dirty_index()],
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
use Side::{Back, Front};

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

//// Main Index

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
    buf: Mmap,

    // For error messages.
    // Log uses this field for error messages.
    pub(crate) path: PathBuf,

    // Logical length. Could be different from `buf.len()`.
    len: u64,

    // OpenOptions
    open_options: OpenOptions,

    // Used by `clear_dirty`.
    clean_root: MemRoot,

    // In-memory entries. The root entry is always in-memory.
    dirty_root: MemRoot,
    dirty_radixes: Vec<MemRadix>,
    dirty_leafs: Vec<MemLeaf>,
    dirty_links: Vec<MemLink>,
    dirty_keys: Vec<MemKey>,
    dirty_ext_keys: Vec<MemExtKey>,

    // Optional checksum table.
    checksum: Checksum,

    // Additional buffer for external keys.
    // Log::sync needs write access to this field.
    pub(crate) key_buf: Arc<dyn ReadonlyBuffer + Send + Sync>,
}

/// Abstraction of the "external key buffer".
///
/// This makes it possible to use non-contiguous memory for a buffer,
/// and expose them as if it's contiguous.
pub trait ReadonlyBuffer {
    /// Get a slice using the given offset.
    fn slice(&self, start: u64, len: u64) -> &[u8];
}

impl<T: AsRef<[u8]>> ReadonlyBuffer for T {
    #[inline]
    fn slice(&self, start: u64, len: u64) -> &[u8] {
        &self.as_ref()[start as usize..(start + len) as usize]
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
    checksum_chunk_size: u64,
    fsync: bool,
    len: Option<u64>,
    write: Option<bool>,
    key_buf: Option<Arc<dyn ReadonlyBuffer + Send + Sync>>,
}

impl OpenOptions {
    /// Create [`OpenOptions`] with default configuration:
    /// - no checksum
    /// - no external key buffer
    /// - no fsync
    /// - read root entry from the end of the file
    /// - open as read-write but fallback to read-only
    pub fn new() -> OpenOptions {
        OpenOptions {
            checksum_chunk_size: 0,
            fsync: false,
            len: None,
            write: None,
            key_buf: None,
        }
    }

    /// Set checksum behavior.
    ///
    /// If `checksum_chunk_size` is set to 0, do not use checksums. Otherwise,
    /// it's the size of a chunk to be checksummed, in bytes. Rounded to `2 ** n`
    /// for performance reasons.
    ///
    /// Disabling checksum can help with performance.
    pub fn checksum_chunk_size(&mut self, checksum_chunk_size: u64) -> &mut Self {
        self.checksum_chunk_size = checksum_chunk_size;
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
        let mut open_options = self.clone();
        let open_result = if self.write == Some(false) {
            fs::OpenOptions::new().read(true).open(path)
        } else {
            fs::OpenOptions::new()
                .read(true)
                .write(true)
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
                if open_result.is_err() {
                    open_options.write = Some(false);
                    fs::OpenOptions::new()
                        .read(true)
                        .open(path)
                        .context(path, "cannot open Index with read-only mode")?
                } else {
                    open_result.unwrap()
                }
            }
        };

        let (mmap, len) = {
            match self.len {
                None => {
                    // Take the lock to read file length, since that decides root entry location.
                    let lock = ScopedFileLock::new(&mut file, false)
                        .context(path, "cannot lock Log to read file length")?;
                    mmap_readonly(lock.as_ref(), None).context(path, "cannot mmap")?
                }
                Some(len) => {
                    // No need to lock for getting file length.
                    mmap_readonly(&file, Some(len)).context(path, "cannot mmap")?
                }
            }
        };

        let checksum_chunk_size = self.checksum_chunk_size;
        let mut checksum = if checksum_chunk_size > 0 {
            Some(ChecksumTable::new(&path)?.fsync(self.fsync))
        } else {
            None
        };

        let (dirty_radixes, clean_root) = if len == 0 {
            // Empty file. Create root radix entry as an dirty entry, and
            // rebuild checksum table (in case it's corrupted).
            let radix_offset = RadixOffset::from_dirty_index(0);
            if let Some(ref mut table) = checksum {
                table.clear();
            }
            let meta = Default::default();
            (vec![MemRadix::default()], MemRoot { radix_offset, meta })
        } else {
            let buf = SimpleIndexBuf(&mmap, path, &checksum);
            // Verify the header byte.
            check_type(&buf, 0, TYPE_HEAD)?;
            // Load root entry from the end of the file (truncated at the logical length).
            (vec![], MemRoot::read_from_end(buf, len)?)
        };

        let key_buf = self.key_buf.clone();
        let dirty_root = clean_root.clone();

        Ok(Index {
            file: Some(file),
            buf: mmap,
            path: path.to_path_buf(),
            open_options,
            clean_root,
            dirty_root,
            dirty_radixes,
            dirty_links: vec![],
            dirty_leafs: vec![],
            dirty_keys: vec![],
            dirty_ext_keys: vec![],
            checksum,
            key_buf: key_buf.unwrap_or(Arc::new(&b""[..])),
            len,
        })
    }

    /// Create an in-memory [`Index`] that skips flushing to disk.
    /// Return an error if `checksum_chunk_size` is not 0.
    pub fn create_in_memory(&self) -> crate::Result<Index> {
        if self.checksum_chunk_size != 0 {
            return Err(crate::Error::programming(
                "checksum_chunk_size is not supported for in-memory Index",
            )
            .into());
        }
        let dirty_radixes = vec![MemRadix::default()];
        let clean_root = {
            let radix_offset = RadixOffset::from_dirty_index(0);
            let meta = Default::default();
            MemRoot { radix_offset, meta }
        };
        let key_buf = self.key_buf.clone();
        let dirty_root = clean_root.clone();

        Ok(Index {
            file: None,
            buf: mmap_empty().infallible()?,
            path: PathBuf::new(),
            open_options: self.clone(),
            clean_root,
            dirty_root,
            dirty_radixes,
            dirty_links: vec![],
            dirty_leafs: vec![],
            dirty_keys: vec![],
            dirty_ext_keys: vec![],
            checksum: None,
            key_buf: key_buf.unwrap_or(Arc::new(&b""[..])),
            len: 0,
        })
    }
}

/// A subset of Index features for read-only accesses.
/// - Provides the main buffer, immutable data serialized on-disk.
/// - Provides the optional checksum checker.
/// - Provides the path (for error message).
trait IndexBuf {
    fn buf(&self) -> &[u8];
    fn checksum(&self) -> &Checksum;
    fn path(&self) -> &Path;

    // Derived methods

    /// Verify checksum for the given range. Internal API used by `*Offset` structs.
    #[inline]
    fn verify_checksum(&self, start: u64, length: u64) -> crate::Result<()> {
        // This method is used in hot code paths. Its instruction size matters.
        // Be sure to run `cargo bench --bench index verified` when changing this
        // function, or the inline attributes.
        if let &Some(ref table) = self.checksum() {
            table.check_range(start, length)
        } else {
            Ok(())
        }
    }

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
    fn checksum(&self) -> &Checksum {
        T::checksum(self)
    }
    fn path(&self) -> &Path {
        T::path(self)
    }
}

impl IndexBuf for Index {
    fn buf(&self) -> &[u8] {
        &self.buf
    }
    fn checksum(&self) -> &Checksum {
        &self.checksum
    }
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Index {
    /// Return a cloned [`Index`] with pending in-memory changes.
    pub fn try_clone(&self) -> crate::Result<Self> {
        self.try_clone_internal(true)
    }

    /// Return a cloned [`Index`] without pending in-memory changes.
    ///
    /// This is logically equivalent to calling `clear_dirty` immediately
    /// on the result after `try_clone`, but potentially cheaper.
    pub fn try_clone_without_dirty(&self) -> crate::Result<Self> {
        self.try_clone_internal(false)
    }

    pub(crate) fn try_clone_internal(&self, copy_dirty: bool) -> crate::Result<Index> {
        let (file, mmap) = match &self.file {
            Some(f) => (
                Some(f.duplicate().context(self.path(), "cannot duplicate")?),
                mmap_readonly(&f, Some(self.len))
                    .context(self.path(), "cannot mmap")?
                    .0,
            ),
            None => {
                assert_eq!(self.len, 0);
                (None, mmap_empty().infallible()?)
            }
        };
        let checksum = match self.checksum {
            Some(ref table) => Some(table.try_clone()?),
            None => None,
        };

        let index = if copy_dirty {
            Index {
                file,
                buf: mmap,
                path: self.path.clone(),
                open_options: self.open_options.clone(),
                clean_root: self.clean_root.clone(),
                dirty_root: self.dirty_root.clone(),
                dirty_keys: self.dirty_keys.clone(),
                dirty_ext_keys: self.dirty_ext_keys.clone(),
                dirty_leafs: self.dirty_leafs.clone(),
                dirty_links: self.dirty_links.clone(),
                dirty_radixes: self.dirty_radixes.clone(),
                checksum,
                key_buf: self.key_buf.clone(),
                len: self.len,
            }
        } else {
            Index {
                file,
                buf: mmap,
                path: self.path.clone(),
                open_options: self.open_options.clone(),
                clean_root: self.clean_root.clone(),
                dirty_root: self.clean_root.clone(),
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
                checksum,
                key_buf: self.key_buf.clone(),
                len: self.len,
            }
        };

        Ok(index)
    }

    /// Get metadata attached to the root node. This is what previously set by
    /// [Index::set_meta].
    pub fn get_meta(&self) -> &[u8] {
        &self.dirty_root.meta
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
    pub fn flush(&mut self) -> crate::Result<u64> {
        if self.open_options.write == Some(false) {
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

        let old_len = self.len;
        let mut new_len = self.len;
        if !self.dirty_root.radix_offset.is_dirty() {
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
                return Err(err.into());
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

            self.dirty_root
                .write_to(&mut buf, &offset_map)
                .infallible()?;
            new_len = buf.len() as u64 + len;
            lock.as_mut()
                .write_all(&buf)
                .context(&path, "cannot write new data to index")?;

            if self.open_options.fsync {
                lock.as_mut().sync_all().context(&path, "cannot sync")?;
            }

            // Remap and update root since length has changed
            let (mmap, mmap_len) =
                mmap_readonly(lock.as_ref(), None).context(&path, "cannot mmap")?;
            self.buf = mmap;

            // 'path' should not have changed.
            debug_assert_eq!(&self.path, &path);

            // This is to workaround the borrow checker.
            let this = SimpleIndexBuf(&self.buf, &path, &None);

            // Sanity check - the length should be expected. Otherwise, the lock
            // is somehow ineffective.
            if mmap_len != new_len {
                return Err(this.corruption("file changed unexpectedly").into());
            }

            if let Some(ref mut table) = self.checksum {
                debug_assert!(self.open_options.checksum_chunk_size > 0);
                let chunk_size_log =
                    63 - (self.open_options.checksum_chunk_size as u64).leading_zeros();
                table.update(chunk_size_log.into())?;
            }

            self.clean_root = MemRoot::read_from_end(this, new_len)?;
        }

        // Outside critical section
        self.len = new_len;
        self.clear_dirty();

        Ok(new_len)
    }

    /// Lookup by `key`. Return [`LinkOffset`].
    ///
    /// To test if the key exists or not, use [Offset::is_null].
    /// To obtain all values, use [`LinkOffset::values`].
    pub fn get<K: AsRef<[u8]>>(&self, key: &K) -> crate::Result<LinkOffset> {
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
                _ => return Err(self.corruption("unexpected type during key lookup").into()),
            }
        }

        // Not found
        Ok(LinkOffset::default())
    }

    /// Scan entries which match the given prefix in base16 form.
    /// Return [`RangeIter`] which allows accesses to keys and values.
    pub fn scan_prefix_base16(
        &self,
        mut base16: impl Iterator<Item = u8>,
    ) -> crate::Result<RangeIter> {
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
                _ => return Err(self.corruption("unexpected type during prefix scan").into()),
            }
        }

        // Not found
        Ok(RangeIter::new(self, front_stack.clone(), front_stack))
    }

    /// Scan entries which match the given prefix in base256 form.
    /// Return [`RangeIter`] which allows accesses to keys and values.
    pub fn scan_prefix<B: AsRef<[u8]>>(&self, prefix: B) -> crate::Result<RangeIter> {
        self.scan_prefix_base16(Base16Iter::from_base256(&prefix))
    }

    /// Scan entries which match the given prefix in hex form.
    /// Return [`RangeIter`] which allows accesses to keys and values.
    pub fn scan_prefix_hex<B: AsRef<[u8]>>(&self, prefix: B) -> crate::Result<RangeIter> {
        // Invalid hex chars will be caught by `radix.child`
        let base16 = prefix.as_ref().iter().cloned().map(single_hex_to_base16);
        self.scan_prefix_base16(base16)
    }

    /// Scans entries whose keys are within the given range.
    ///
    /// Returns a double-ended iterator, which provides accesses to keys and
    /// values.
    pub fn range<'a>(&self, range: impl RangeBounds<&'a [u8]>) -> crate::Result<RangeIter> {
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

        let front_stack = self.iter_stack_by_bound(range.start_bound(), Front)?;
        let back_stack = self.iter_stack_by_bound(range.end_bound(), Back)?;
        Ok(RangeIter::new(self, front_stack, back_stack))
    }

    /// Insert a key-value pair. The value will be the head of the linked list.
    /// That is, `get(key).values().first()` will return the newly inserted
    /// value.
    pub fn insert<K: AsRef<[u8]>>(&mut self, key: &K, value: u64) -> crate::Result<()> {
        self.insert_advanced(InsertKey::Embed(key.as_ref()), value.into(), None)
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
    pub fn insert_advanced(
        &mut self,
        key: InsertKey,
        value: u64,
        link: Option<LinkOffset>,
    ) -> crate::Result<()> {
        let mut offset: Offset = self.dirty_root.radix_offset.into();
        let mut step = 0;
        let (key, key_buf_offset) = match key {
            InsertKey::Embed(k) => (k, None),
            InsertKey::Reference((start, len)) => {
                let key = self.key_buf.as_ref().slice(start, len);
                // UNSAFE NOTICE: `key` is valid as long as `self.key_buf` is valid. `self.key_buf`
                // won't be changed. So `self` can still be mutable without a read-only
                // relationship with `key`.
                let detached_key = unsafe { &*(key as (*const [u8])) };
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
                            let old_link_offset = radix.link_offset(self)?;
                            let new_link_offset =
                                link.unwrap_or(old_link_offset).create(self, value);
                            radix.set_link(self, new_link_offset);
                            return Ok(());
                        }
                        Some(x) => {
                            let next_offset = radix.child(self, x)?;
                            if next_offset.is_null() {
                                // "key" is longer than existing ones. Create key and leaf entries.
                                let link_offset =
                                    link.unwrap_or(LinkOffset::default()).create(self, value);
                                let key_offset = self.create_key(key, key_buf_offset);
                                let leaf_offset =
                                    LeafOffset::create(self, link_offset, key_offset.into());
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
                    let (old_key, link_offset) = {
                        let (old_key, link_offset) = leaf.key_and_link_offset(self)?;
                        // Detach "old_key" from "self".
                        // About safety: This is to avoid a memory copy / allocation.
                        // `old_key` are only valid before `dirty_*keys` being resized.
                        // `old_iter` (used by `split_leaf`) and `old_key` are not used
                        // after creating a key. So it's safe to not copy it.
                        let detached_key = unsafe { &*(old_key as (*const [u8])) };
                        (detached_key, link_offset)
                    };
                    if old_key == key.as_ref() {
                        // Key matched. Need to copy leaf entry.
                        let new_link_offset = link.unwrap_or(link_offset).create(self, value);
                        let new_leaf_offset = leaf.set_link(self, new_link_offset)?;
                        last_radix.set_child(self, last_child, new_leaf_offset.into());
                    } else {
                        // Key mismatch. Do a leaf split.
                        let new_link_offset =
                            link.unwrap_or(LinkOffset::default()).create(self, value);
                        self.split_leaf(
                            leaf,
                            old_key,
                            key.as_ref(),
                            key_buf_offset,
                            step,
                            last_radix,
                            last_child,
                            link_offset,
                            new_link_offset,
                        )?;
                    }
                    return Ok(());
                }
                _ => return Err(self.corruption("unexpected type during insertion").into()),
            }

            step += 1;
        }
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
                        let state = match inclusive {
                            true => state.step(side).unwrap(),
                            false => state,
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
                _ => return Err(self.corruption("unexpected type following prefix").into()),
            }
        }

        // Prefix does not exist. The stack ends with a RadixChild state that
        // points to nothing.
        Ok(stack)
    }

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

            if b2.is_none() {
                // Example 2. new_key is a prefix of old_key. A new leaf is not needed.
                radix.link_offset = new_link_offset;
                completed = true;
            } else if b1 != b2 {
                // Example 1 and Example 3. A new leaf is needed.
                let new_key_offset = self.create_key(new_key, key_buf_offset);
                let new_leaf_offset = LeafOffset::create(self, new_link_offset, new_key_offset);
                radix.offsets[b2.unwrap() as usize] = new_leaf_offset.into();
                completed = true;
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

//// Debug Formatter

impl Debug for Offset {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        if self.is_null() {
            write!(f, "None")
        } else if self.is_dirty() {
            let path = Path::new("<dummy>");
            let dummy = SimpleIndexBuf(b"", &path, &None);
            match self.to_typed(dummy).unwrap() {
                TypedOffset::Radix(x) => x.fmt(f),
                TypedOffset::Leaf(x) => x.fmt(f),
                TypedOffset::Link(x) => x.fmt(f),
                TypedOffset::Key(x) => x.fmt(f),
                TypedOffset::ExtKey(x) => x.fmt(f),
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

impl Debug for Index {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "Index {{ len: {}, root: {:?} }}\n",
            self.buf.len(),
            self.dirty_root.radix_offset
        )?;

        // On-disk entries
        let offset_map = OffsetMap::default();
        let mut buf = Vec::with_capacity(self.buf.len());
        buf.push(TYPE_HEAD);
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
                    write!(f, "{:?}\n", e)?;
                }
                TYPE_LEAF => {
                    let e = MemLeaf::read_from(self, i).expect("read");
                    e.write_noninline_to(&mut buf, &offset_map).expect("write");
                    write!(f, "{:?}\n", e)?;
                }
                TYPE_INLINE_LEAF => {
                    let e = MemLeaf::read_from(self, i).expect("read");
                    write!(f, "Inline{:?}\n", e)?;
                    // Just skip the type int byte so we can parse inlined structures.
                    buf.push(TYPE_INLINE_LEAF);
                }
                TYPE_LINK => {
                    let e = MemLink::read_from(self, i).unwrap();
                    e.write_to(&mut buf, &offset_map).expect("write");
                    write!(f, "{:?}\n", e)?;
                }
                TYPE_KEY => {
                    let e = MemKey::read_from(self, i).expect("read");
                    e.write_to(&mut buf, &offset_map).expect("write");
                    write!(f, "{:?}\n", e)?;
                }
                TYPE_EXT_KEY => {
                    let e = MemExtKey::read_from(self, i).expect("read");
                    e.write_to(&mut buf, &offset_map).expect("write");
                    write!(f, "{:?}\n", e)?;
                }
                TYPE_ROOT => {
                    let e = MemRoot::read_from(self, i).expect("read");
                    e.write_to(&mut buf, &offset_map).expect("write");
                    write!(f, "{:?}\n", e)?;
                }
                _ => {
                    write!(f, "Broken Data!\n")?;
                    break;
                }
            }
        }

        if buf.len() > 1 && self.buf[..] != buf[..] {
            return write!(f, "Inconsistent Data!\n");
        }

        // In-memory entries
        for (i, e) in self.dirty_radixes.iter().enumerate() {
            write!(f, "Radix[{}]: ", i)?;
            write!(f, "{:?}\n", e)?;
        }

        for (i, e) in self.dirty_leafs.iter().enumerate() {
            write!(f, "Leaf[{}]: ", i)?;
            write!(f, "{:?}\n", e)?;
        }

        for (i, e) in self.dirty_links.iter().enumerate() {
            write!(f, "Link[{}]: ", i)?;
            write!(f, "{:?}\n", e)?;
        }

        for (i, e) in self.dirty_keys.iter().enumerate() {
            write!(f, "Key[{}]: ", i)?;
            write!(f, "{:?}\n", e)?;
        }

        for (i, e) in self.dirty_ext_keys.iter().enumerate() {
            write!(f, "ExtKey[{}]: {:?}\n", i, e)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;
    use std::collections::{BTreeSet, HashMap};
    use std::fs::File;
    use std::io::prelude::*;
    use tempfile::tempdir;

    fn open_opts() -> OpenOptions {
        let mut opts = OpenOptions::new();
        // Use 1 as checksum chunk size to make sure checksum check covers necessary bytes.
        opts.checksum_chunk_size(1);
        opts
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
    fn test_distinct_one_byte_keys() {
        let dir = tempdir().unwrap();
        let mut index = open_opts().open(dir.path().join("a")).expect("open");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 1, root: Radix[0] }\n\
             Radix[0]: Radix { link: None }\n"
        );

        index.insert(&[], 55).expect("update");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 1, root: Radix[0] }\n\
             Radix[0]: Radix { link: Link[0] }\n\
             Link[0]: Link { value: 55, next: None }\n"
        );

        index.insert(&[0x12], 77).expect("update");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 1, root: Radix[0] }\n\
             Radix[0]: Radix { link: Link[0], 1: Leaf[0] }\n\
             Leaf[0]: Leaf { key: Key[0], link: Link[1] }\n\
             Link[0]: Link { value: 55, next: None }\n\
             Link[1]: Link { value: 77, next: None }\n\
             Key[0]: Key { key: 12 }\n"
        );

        let link = index.get(&[0x12]).expect("get");
        index
            .insert_advanced(InsertKey::Embed(&[0x34]), 99, link.into())
            .expect("update");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 1, root: Radix[0] }\n\
             Radix[0]: Radix { link: Link[0], 1: Leaf[0], 3: Leaf[1] }\n\
             Leaf[0]: Leaf { key: Key[0], link: Link[1] }\n\
             Leaf[1]: Leaf { key: Key[1], link: Link[2] }\n\
             Link[0]: Link { value: 55, next: None }\n\
             Link[1]: Link { value: 77, next: None }\n\
             Link[2]: Link { value: 99, next: Link[1] }\n\
             Key[0]: Key { key: 12 }\n\
             Key[1]: Key { key: 34 }\n"
        );
    }

    #[test]
    fn test_distinct_one_byte_keys_flush() {
        let dir = tempdir().unwrap();
        let mut index = open_opts().open(dir.path().join("a")).expect("open");

        // 1st flush.
        assert_eq!(index.flush().expect("flush"), 9);
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 9, root: Disk[1] }\n\
             Disk[1]: Radix { link: None }\n\
             Disk[5]: Root { radix: Disk[1] }\n"
        );

        // Mixed on-disk and in-memory state.
        index.insert(&[], 55).expect("update");
        index.insert(&[0x12], 77).expect("update");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 9, root: Radix[0] }\n\
             Disk[1]: Radix { link: None }\n\
             Disk[5]: Root { radix: Disk[1] }\n\
             Radix[0]: Radix { link: Link[0], 1: Leaf[0] }\n\
             Leaf[0]: Leaf { key: Key[0], link: Link[1] }\n\
             Link[0]: Link { value: 55, next: None }\n\
             Link[1]: Link { value: 77, next: None }\n\
             Key[0]: Key { key: 12 }\n"
        );

        // After 2nd flush. There are 2 roots.
        let link = index.get(&[0x12]).expect("get");
        index
            .insert_advanced(InsertKey::Embed(&[0x34]), 99, link.into())
            .expect("update");
        index.flush().expect("flush");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 50, root: Disk[30] }\n\
             Disk[1]: Radix { link: None }\n\
             Disk[5]: Root { radix: Disk[1] }\n\
             Disk[9]: Key { key: 12 }\n\
             Disk[12]: Key { key: 34 }\n\
             Disk[15]: Link { value: 55, next: None }\n\
             Disk[18]: Link { value: 77, next: None }\n\
             Disk[21]: Link { value: 99, next: Disk[18] }\n\
             Disk[24]: Leaf { key: Disk[9], link: Disk[18] }\n\
             Disk[27]: Leaf { key: Disk[12], link: Disk[21] }\n\
             Disk[30]: Radix { link: Disk[15], 1: Disk[24], 3: Disk[27] }\n\
             Disk[46]: Root { radix: Disk[30] }\n"
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
            "Index { len: 1, root: Radix[0] }\n\
             Radix[0]: Radix { link: None, 1: Leaf[0] }\n\
             Leaf[0]: Leaf { key: Key[0], link: Link[0] }\n\
             Link[0]: Link { value: 5, next: None }\n\
             Key[0]: Key { key: 12 34 }\n"
        );
        index.insert(&[0x12, 0x78], 7).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 1, root: Radix[0] }\n\
             Radix[0]: Radix { link: None, 1: Radix[1] }\n\
             Radix[1]: Radix { link: None, 2: Radix[2] }\n\
             Radix[2]: Radix { link: None, 3: Leaf[0], 7: Leaf[1] }\n\
             Leaf[0]: Leaf { key: Key[0], link: Link[0] }\n\
             Leaf[1]: Leaf { key: Key[1], link: Link[1] }\n\
             Link[0]: Link { value: 5, next: None }\n\
             Link[1]: Link { value: 7, next: None }\n\
             Key[0]: Key { key: 12 34 }\n\
             Key[1]: Key { key: 12 78 }\n"
        );

        // Example 2: new key is a prefix of the old key
        let mut index = open_opts().open(dir.path().join("a")).expect("open");
        index.insert(&[0x12, 0x34], 5).expect("insert");
        index.insert(&[0x12], 7).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 1, root: Radix[0] }\n\
             Radix[0]: Radix { link: None, 1: Radix[1] }\n\
             Radix[1]: Radix { link: None, 2: Radix[2] }\n\
             Radix[2]: Radix { link: Link[1], 3: Leaf[0] }\n\
             Leaf[0]: Leaf { key: Key[0], link: Link[0] }\n\
             Link[0]: Link { value: 5, next: None }\n\
             Link[1]: Link { value: 7, next: None }\n\
             Key[0]: Key { key: 12 34 }\n"
        );

        // Example 3: old key is a prefix of the new key
        let mut index = open_opts().open(dir.path().join("a")).expect("open");
        index.insert(&[0x12], 5).expect("insert");
        index.insert(&[0x12, 0x78], 7).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 1, root: Radix[0] }\n\
             Radix[0]: Radix { link: None, 1: Radix[1] }\n\
             Radix[1]: Radix { link: None, 2: Radix[2] }\n\
             Radix[2]: Radix { link: Link[0], 7: Leaf[1] }\n\
             Leaf[0]: Leaf (unused)\n\
             Leaf[1]: Leaf { key: Key[1], link: Link[1] }\n\
             Link[0]: Link { value: 5, next: None }\n\
             Link[1]: Link { value: 7, next: None }\n\
             Key[0]: Key (unused)\n\
             Key[1]: Key { key: 12 78 }\n"
        );

        // Same key. Multiple values.
        let mut index = open_opts().open(dir.path().join("a")).expect("open");
        index.insert(&[0x12], 5).expect("insert");
        index.insert(&[0x12], 7).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 1, root: Radix[0] }\n\
             Radix[0]: Radix { link: None, 1: Leaf[0] }\n\
             Leaf[0]: Leaf { key: Key[0], link: Link[1] }\n\
             Link[0]: Link { value: 5, next: None }\n\
             Link[1]: Link { value: 7, next: Link[0] }\n\
             Key[0]: Key { key: 12 }\n"
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
            "Index { len: 23, root: Disk[11] }\n\
             Disk[1]: Key { key: 12 34 }\n\
             Disk[5]: Link { value: 5, next: None }\n\
             Disk[8]: Leaf { key: Disk[1], link: Disk[5] }\n\
             Disk[11]: Radix { link: None, 1: Disk[8] }\n\
             Disk[19]: Root { radix: Disk[11] }\n"
        );
        index.insert(&[0x12, 0x78], 7).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 23, root: Radix[0] }\n\
             Disk[1]: Key { key: 12 34 }\n\
             Disk[5]: Link { value: 5, next: None }\n\
             Disk[8]: Leaf { key: Disk[1], link: Disk[5] }\n\
             Disk[11]: Radix { link: None, 1: Disk[8] }\n\
             Disk[19]: Root { radix: Disk[11] }\n\
             Radix[0]: Radix { link: None, 1: Radix[1] }\n\
             Radix[1]: Radix { link: None, 2: Radix[2] }\n\
             Radix[2]: Radix { link: None, 3: Disk[8], 7: Leaf[0] }\n\
             Leaf[0]: Leaf { key: Key[0], link: Link[0] }\n\
             Link[0]: Link { value: 7, next: None }\n\
             Key[0]: Key { key: 12 78 }\n"
        );

        // Example 2: new key is a prefix of the old key
        let mut index = open_opts().open(dir.path().join("2")).expect("open");
        index.insert(&[0x12, 0x34], 5).expect("insert");
        index.flush().expect("flush");
        index.insert(&[0x12], 7).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 23, root: Radix[0] }\n\
             Disk[1]: Key { key: 12 34 }\n\
             Disk[5]: Link { value: 5, next: None }\n\
             Disk[8]: Leaf { key: Disk[1], link: Disk[5] }\n\
             Disk[11]: Radix { link: None, 1: Disk[8] }\n\
             Disk[19]: Root { radix: Disk[11] }\n\
             Radix[0]: Radix { link: None, 1: Radix[1] }\n\
             Radix[1]: Radix { link: None, 2: Radix[2] }\n\
             Radix[2]: Radix { link: Link[0], 3: Disk[8] }\n\
             Link[0]: Link { value: 7, next: None }\n"
        );

        // Example 3: old key is a prefix of the new key
        // Only one flush - only one key is written.
        let mut index = open_opts().open(dir.path().join("3a")).expect("open");
        index.insert(&[0x12], 5).expect("insert");
        index.insert(&[0x12, 0x78], 7).expect("insert");
        index.flush().expect("flush");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 46, root: Disk[34] }\n\
             Disk[1]: Key { key: 12 78 }\n\
             Disk[5]: Link { value: 5, next: None }\n\
             Disk[8]: Link { value: 7, next: None }\n\
             Disk[11]: Leaf { key: Disk[1], link: Disk[8] }\n\
             Disk[14]: Radix { link: Disk[5], 7: Disk[11] }\n\
             Disk[26]: Radix { link: None, 2: Disk[14] }\n\
             Disk[34]: Radix { link: None, 1: Disk[26] }\n\
             Disk[42]: Root { radix: Disk[34] }\n"
        );

        // With two flushes - the old key cannot be removed since it was written.
        let mut index = open_opts().open(dir.path().join("3b")).expect("open");
        index.insert(&[0x12], 5).expect("insert");
        index.flush().expect("flush");
        index.insert(&[0x12, 0x78], 7).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 22, root: Radix[0] }\n\
             Disk[1]: Key { key: 12 }\n\
             Disk[4]: Link { value: 5, next: None }\n\
             Disk[7]: Leaf { key: Disk[1], link: Disk[4] }\n\
             Disk[10]: Radix { link: None, 1: Disk[7] }\n\
             Disk[18]: Root { radix: Disk[10] }\n\
             Radix[0]: Radix { link: None, 1: Radix[1] }\n\
             Radix[1]: Radix { link: None, 2: Radix[2] }\n\
             Radix[2]: Radix { link: Disk[4], 7: Leaf[0] }\n\
             Leaf[0]: Leaf { key: Key[0], link: Link[0] }\n\
             Link[0]: Link { value: 7, next: None }\n\
             Key[0]: Key { key: 12 78 }\n"
        );

        // Same key. Multiple values.
        let mut index = open_opts().open(dir.path().join("4")).expect("open");
        index.insert(&[0x12], 5).expect("insert");
        index.flush().expect("flush");
        index.insert(&[0x12], 7).expect("insert");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 22, root: Radix[0] }\n\
             Disk[1]: Key { key: 12 }\n\
             Disk[4]: Link { value: 5, next: None }\n\
             Disk[7]: Leaf { key: Disk[1], link: Disk[4] }\n\
             Disk[10]: Radix { link: None, 1: Disk[7] }\n\
             Disk[18]: Root { radix: Disk[10] }\n\
             Radix[0]: Radix { link: None, 1: Leaf[0] }\n\
             Leaf[0]: Leaf { key: Disk[1], link: Link[0] }\n\
             Link[0]: Link { value: 7, next: Disk[4] }\n"
        );
    }

    #[test]
    fn test_external_keys() {
        let buf = Arc::new(vec![0x12u8, 0x34, 0x56, 0x78, 0x9a, 0xbc]);
        let dir = tempdir().unwrap();
        let mut index = open_opts()
            .key_buf(Some(buf.clone()))
            .open(dir.path().join("a"))
            .expect("open");
        index
            .insert_advanced(InsertKey::Reference((1, 2)), 55, None)
            .expect("insert");
        index.flush().expect("flush");
        index
            .insert_advanced(InsertKey::Reference((1, 3)), 77, None)
            .expect("insert");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 20, root: Radix[0] }\n\
             Disk[1]: InlineLeaf { key: Disk[2], link: Disk[5] }\n\
             Disk[2]: ExtKey { start: 1, len: 2 }\n\
             Disk[5]: Link { value: 55, next: None }\n\
             Disk[8]: Radix { link: None, 3: Disk[1] }\n\
             Disk[16]: Root { radix: Disk[8] }\n\
             Radix[0]: Radix { link: None, 3: Radix[1] }\n\
             Radix[1]: Radix { link: None, 4: Radix[2] }\n\
             Radix[2]: Radix { link: None, 5: Radix[3] }\n\
             Radix[3]: Radix { link: None, 6: Radix[4] }\n\
             Radix[4]: Radix { link: Disk[5], 7: Leaf[0] }\n\
             Leaf[0]: Leaf { key: ExtKey[0], link: Link[0] }\n\
             Link[0]: Link { value: 77, next: None }\n\
             ExtKey[0]: ExtKey { start: 1, len: 3 }\n"
        );
    }

    #[test]
    fn test_inline_leafs() {
        let buf = Arc::new(vec![0x12u8, 0x34, 0x56, 0x78, 0x9a, 0xbc]);
        let dir = tempdir().unwrap();
        let mut index = open_opts()
            .key_buf(Some(buf.clone()))
            .open(dir.path().join("a"))
            .expect("open");

        // New entry. Should be inlined.
        index
            .insert_advanced(InsertKey::Reference((1, 1)), 55, None)
            .unwrap();
        index.flush().expect("flush");

        // Independent leaf. Should also be inlined.
        index
            .insert_advanced(InsertKey::Reference((2, 1)), 77, None)
            .unwrap();
        index.flush().expect("flush");

        // The link with 88 should refer to the inlined leaf 77.
        index
            .insert_advanced(InsertKey::Reference((2, 1)), 88, None)
            .unwrap();
        index.flush().expect("flush");

        // Not inlined because dependent link was not written first.
        // (could be optimized in the future)
        index
            .insert_advanced(InsertKey::Reference((3, 1)), 99, None)
            .unwrap();
        index
            .insert_advanced(InsertKey::Reference((3, 1)), 100, None)
            .unwrap();
        index.flush().expect("flush");

        assert_eq!(
            format!("{:?}", index),
            "Index { len: 97, root: Disk[77] }\n\
             Disk[1]: InlineLeaf { key: Disk[2], link: Disk[5] }\n\
             Disk[2]: ExtKey { start: 1, len: 1 }\n\
             Disk[5]: Link { value: 55, next: None }\n\
             Disk[8]: Radix { link: None, 3: Disk[1] }\n\
             Disk[16]: Root { radix: Disk[8] }\n\
             Disk[20]: InlineLeaf { key: Disk[21], link: Disk[24] }\n\
             Disk[21]: ExtKey { start: 2, len: 1 }\n\
             Disk[24]: Link { value: 77, next: None }\n\
             Disk[27]: Radix { link: None, 3: Disk[1], 5: Disk[20] }\n\
             Disk[39]: Root { radix: Disk[27] }\n\
             Disk[43]: Link { value: 88, next: Disk[24] }\n\
             Disk[46]: Leaf { key: Disk[21], link: Disk[43] }\n\
             Disk[49]: Radix { link: None, 3: Disk[1], 5: Disk[46] }\n\
             Disk[61]: Root { radix: Disk[49] }\n\
             Disk[65]: ExtKey { start: 3, len: 1 }\n\
             Disk[68]: Link { value: 99, next: None }\n\
             Disk[71]: Link { value: 100, next: Disk[68] }\n\
             Disk[74]: Leaf { key: Disk[65], link: Disk[71] }\n\
             Disk[77]: Radix { link: None, 3: Disk[1], 5: Disk[46], 7: Disk[74] }\n\
             Disk[93]: Root { radix: Disk[77] }\n"
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
            .checksum_chunk_size(0)
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
    fn test_checksum_bitflip() {
        let dir = tempdir().unwrap();

        // Debug build is much slower than release build. Limit the key length to 1-byte.
        #[cfg(debug_assertions)]
        let keys = vec![vec![0x13], vec![0x17], vec![]];

        // Release build can afford 2-byte key test.
        #[cfg(not(debug_assertions))]
        let keys = vec![
            vec![0x12, 0x34],
            vec![0x12, 0x78],
            vec![0x34, 0x56],
            vec![0x34],
            vec![0x78],
            vec![0x78, 0x9a],
        ];

        let bytes = {
            let mut index = open_opts().open(dir.path().join("a")).expect("open");

            for (i, key) in keys.iter().enumerate() {
                index.insert(key, i as u64).expect("insert");
                index.insert(key, (i as u64) << 50).expect("insert");
            }
            index.flush().expect("flush");

            // Read the raw bytes of the index content
            let mut f = File::open(dir.path().join("a")).expect("open");
            let mut buf = vec![];
            f.read_to_end(&mut buf).expect("read");
            buf

            // Drop `index` here. This would unmap files so File::create below
            // can work on Windows.
        };

        fn is_corrupted(index: &Index, key: &[u8]) -> bool {
            let link = index.get(&key);
            match link {
                Err(_) => true,
                Ok(link) => link.values(&index).any(|v| v.is_err()),
            }
        }

        // Every bit change should trigger errors when reading all contents
        for i in 0..(bytes.len() * 8) {
            let mut bytes = bytes.clone();
            bytes[i / 8] ^= 1u8 << (i % 8);
            let mut f = File::create(dir.path().join("a")).expect("create");
            f.write_all(&bytes).expect("write");

            let index = open_opts().open(dir.path().join("a"));
            let detected = match index {
                Err(_) => true,
                Ok(index) => {
                    #[cfg(debug_assertions)]
                    let range = 0;
                    #[cfg(not(debug_assertions))]
                    let range = 0x10000;

                    (0..range).any(|key_int| {
                        let key = [(key_int >> 8) as u8, (key_int & 0xff) as u8];
                        is_corrupted(&index, &key)
                    }) || (0..0x100).any(|key_int| {
                        let key = [key_int as u8];
                        is_corrupted(&index, &key)
                    }) || is_corrupted(&index, &[])
                }
            };
            assert!(detected, "bit flip at {} is not detected", i);
        }
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
    fn iter_to_keys(index: &Index, keys: &Vec<&[u8]>, iter: &RangeIter) -> Vec<Vec<u8>> {
        let it_forward = iter.clone_with_index(index);
        let it_backward = iter.clone_with_index(index);
        let mut it_both_ends = iter.clone_with_index(index);

        let extract = |v: crate::Result<(Cow<'_, [u8]>, LinkOffset)>| -> Vec<u8> {
            let (key, link_offset) = v.unwrap();
            let key = key.as_ref();
            // Verify link_offset is correct
            let ids: Vec<u64> = link_offset
                .values(&index)
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
            for b in [0x00, 0x77, 0xff].iter().cloned() {
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
            vec![
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

        index.set_meta(&vec![42]);
        index.insert(&"foo", 1).unwrap();
        index.flush().unwrap();

        index.set_meta(&vec![43]);
        index.insert(&"bar", 2).unwrap();
        index.clear_dirty();

        assert_eq!(index.get_meta(), [42]);
        assert!(index.get(&"bar").unwrap().is_null());
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

            map.iter().all(|(key, value)| {
                let link_offset = index.get(key).expect("lookup");
                assert!(!link_offset.is_null());
                link_offset.value_and_next(&index).unwrap().0 == *value
            })
        }

        fn test_multiple_values(map: HashMap<Vec<u8>, Vec<u64>>) -> bool {
            let dir = tempdir().unwrap();
            let mut index = open_opts().open(dir.path().join("a")).expect("open");
            let mut index_mem = open_opts().checksum_chunk_size(0).create_in_memory().unwrap();

            for (key, values) in &map {
                for value in values.iter().rev() {
                    index.insert(key, *value).expect("insert");
                    index_mem.insert(key, *value).expect("insert");
                }
                if values.len() == 0 {
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
    }
}

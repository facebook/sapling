//! [u8] -> [u64] mapping. Insertion only.
//!
//! The index could be backed by a combination of an on-disk file, and in-memory content. Changes
//! to the index will be buffered in memory forever until an explicit flush. Internally, the index
//! uses base16 radix tree for keys and linked list of values, though it's possible to extend the
//! format to support other kinds of trees and values.
//!
//! File format:
//!
//! ```ignore
//! INDEX       := HEADER + ENTRY_LIST
//! HEADER      := '\0'  (takes offset 0, so 0 is not a valid offset for ENTRY)
//! ENTRY_LIST  := RADIX | ENTRY_LIST + ENTRY
//! ENTRY       := RADIX | LEAF | LINK | KEY | ROOT
//! RADIX       := '\2' + JUMP_TABLE (16 bytes) + PTR(LINK) + PTR(RADIX | LEAF) * N
//! LEAF        := '\3' + PTR(KEY) + PTR(LINK)
//! LINK        := '\4' + VLQ(VALUE) + PTR(NEXT_LINK | NULL)
//! KEY         := '\5' + VLQ(KEY_LEN) + KEY_BYTES
//! ROOT        := '\1' + PTR(RADIX) + ROOT_LEN (1 byte)
//!
//! PTR(ENTRY)  := VLQ(the offset of ENTRY)
//! ```
//!
//! Some notes about the format:
//!
//! - A "RADIX" entry has 16 children. This is mainly for source control hex hashes. The "N"
//!   in a radix entry could be less than 16 if some of the children are missing (ex. offset = 0).
//!   The corresponding jump table bytes of missing children are 0s. If child i exists, then
//!   `jumptable[i]` is the relative (to the beginning of radix entry) offset of PTR(child offset).
//! - A "ROOT" entry its length recorded as the last byte. Normally the root entry is written
//!   at the end. This makes it easier for the caller - it does not have to record the position
//!   of the root entry. The caller could optionally provide a root location.
//! - An entry has a 1 byte "type". This makes it possible to do a linear scan from the
//!   beginning of the file, instead of having to go through a root. Potentially useful for
//!   recovery purpose, or adding new entry types (ex. tree entries other than the 16-children
//!   radix entry, value entries that are not u64 linked list, key entries that refers external
//!   buffer).
//! - The "JUMP_TABLE" in "RADIX" entry stores relative offsets to the actual value of
//!   RADIX/LEAF offsets. It has redundant information. The more compact form is a 2-byte
//!   (16-bit) bitmask but that hurts lookup performance.

use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::io::ErrorKind::InvalidData;
use std::path::Path;

use base16::Base16Iter;
use lock::ScopedFileLock;
use utils::mmap_readonly;

use memmap::Mmap;
use vlqencoding::{VLQDecodeAt, VLQEncode};

//// Structures related to file format

#[derive(Clone, PartialEq, Default)]
struct Radix {
    pub offsets: [u64; 16],
    pub link_offset: u64,
}

#[derive(Clone, PartialEq)]
struct Leaf {
    pub key_offset: u64,
    pub link_offset: u64,
}

#[derive(Clone, PartialEq)]
struct Key {
    pub key: Vec<u8>, // base256
}

#[derive(Clone, PartialEq)]
struct Link {
    pub value: u64,
    pub next_link_offset: u64,
}

#[derive(Clone, PartialEq)]
struct Root {
    pub radix_offset: u64,
}

//// Serialization

// Offsets that are >= DIRTY_OFFSET refer to in-memory entries that haven't been
// written to disk. Offsets < DIRTY_OFFSET are on-disk offsets.
const DIRTY_OFFSET: u64 = 1u64 << 63;

const TYPE_HEAD: u8 = 0;
const TYPE_ROOT: u8 = 1;
const TYPE_RADIX: u8 = 2;
const TYPE_LEAF: u8 = 3;
const TYPE_LINK: u8 = 4;
const TYPE_KEY: u8 = 5;

/// Convert a possibly "dirty" offset to a non-dirty offset.
fn translate_offset(v: u64, offset_map: &HashMap<u64, u64>) -> u64 {
    if v >= DIRTY_OFFSET {
        // Should always find a value. Otherwise it's a programming error about write order.
        *offset_map.get(&v).unwrap()
    } else {
        v
    }
}

/// Check type for an on-disk entry
fn check_type(buf: &[u8], offset: usize, expected: u8) -> io::Result<()> {
    let typeint = *(buf.get(offset).ok_or(InvalidData)?);
    if typeint != expected {
        Err(InvalidData.into())
    } else {
        Ok(())
    }
}

impl Radix {
    fn read_from<B: AsRef<[u8]>>(buf: B, offset: u64) -> io::Result<Self> {
        let buf = buf.as_ref();
        let offset = offset as usize;
        let mut pos = 0;

        check_type(buf, offset, TYPE_RADIX)?;
        pos += 1;

        let jumptable = buf.get(offset + pos..offset + pos + 16).ok_or(InvalidData)?;
        pos += 16;

        let (link_offset, len) = buf.read_vlq_at(offset + pos)?;
        pos += len;

        let mut offsets = [0; 16];
        for i in 0..16 {
            if jumptable[i] != 0 {
                if jumptable[i] as usize != pos {
                    return Err(InvalidData.into());
                }
                let (v, len) = buf.read_vlq_at(offset + pos)?;
                offsets[i] = v;
                pos += len;
            }
        }

        Ok(Radix {
            offsets,
            link_offset,
        })
    }

    fn write_to<W: Write>(&self, writer: &mut W, offset_map: &HashMap<u64, u64>) -> io::Result<()> {
        // Approximate size good enough for an average radix entry
        let mut buf = Vec::with_capacity(1 + 16 + 5 * 17);

        buf.write_all(&[TYPE_RADIX])?;
        buf.write_all(&[0u8; 16])?;
        buf.write_vlq(translate_offset(self.link_offset, offset_map))?;

        for i in 0..16 {
            let v = self.offsets[i];
            if v != 0 {
                let v = translate_offset(v, offset_map);
                buf[1 + i] = buf.len() as u8; // update jump table
                buf.write_vlq(v)?;
            }
        }

        writer.write_all(&buf)
    }
}

impl Leaf {
    fn read_from<B: AsRef<[u8]>>(buf: B, offset: u64) -> io::Result<Self> {
        let buf = buf.as_ref();
        let offset = offset as usize;
        check_type(buf, offset, TYPE_LEAF)?;
        let (key_offset, len) = buf.read_vlq_at(offset + 1)?;
        let (link_offset, _) = buf.read_vlq_at(offset + len + 1)?;
        Ok(Leaf {
            key_offset,
            link_offset,
        })
    }

    fn write_to<W: Write>(&self, writer: &mut W, offset_map: &HashMap<u64, u64>) -> io::Result<()> {
        writer.write_all(&[TYPE_LEAF])?;
        writer.write_vlq(translate_offset(self.key_offset, offset_map))?;
        writer.write_vlq(translate_offset(self.link_offset, offset_map))?;
        Ok(())
    }
}

impl Link {
    fn read_from<B: AsRef<[u8]>>(buf: B, offset: u64) -> io::Result<Self> {
        let buf = buf.as_ref();
        let offset = offset as usize;
        check_type(buf, offset, TYPE_LINK)?;
        let (value, len) = buf.read_vlq_at(offset + 1)?;
        let (next_link_offset, _) = buf.read_vlq_at(offset + len + 1)?;
        Ok(Link {
            value,
            next_link_offset,
        })
    }

    fn write_to<W: Write>(&self, writer: &mut W, offset_map: &HashMap<u64, u64>) -> io::Result<()> {
        writer.write_all(&[TYPE_LINK])?;
        writer.write_vlq(self.value)?;
        writer.write_vlq(translate_offset(self.next_link_offset, offset_map))?;
        Ok(())
    }
}

impl Key {
    fn read_from<B: AsRef<[u8]>>(buf: B, offset: u64) -> io::Result<Self> {
        let buf = buf.as_ref();
        let offset = offset as usize;
        check_type(buf, offset, TYPE_KEY)?;
        let (key_len, len): (usize, _) = buf.read_vlq_at(offset + 1)?;
        let key = Vec::from(buf.get(offset + 1 + len..offset + 1 + len + key_len)
            .ok_or(InvalidData)?);
        Ok(Key { key })
    }

    fn write_to<W: Write>(&self, writer: &mut W, offset_map: &HashMap<u64, u64>) -> io::Result<()> {
        writer.write_all(&[TYPE_KEY])?;
        writer.write_vlq(self.key.len())?;
        writer.write_all(&self.key)?;
        Ok(())
    }
}

impl Root {
    fn read_from<B: AsRef<[u8]>>(buf: B, offset: u64) -> io::Result<Self> {
        let buf = buf.as_ref();
        let offset = offset as usize;
        check_type(buf, offset, TYPE_ROOT)?;
        let (radix_offset, len1) = buf.read_vlq_at(offset + 1)?;
        let (len, _): (usize, _) = buf.read_vlq_at(offset + 1 + len1)?;
        if len == 1 + len1 + 1 {
            Ok(Root { radix_offset })
        } else {
            Err(InvalidData.into())
        }
    }

    fn read_from_end<B: AsRef<[u8]>>(buf: B, end: u64) -> io::Result<Self> {
        if end > 1 {
            let (size, _): (u64, _) = buf.as_ref().read_vlq_at(end as usize - 1)?;
            Self::read_from(buf, end - size)
        } else {
            Err(InvalidData.into())
        }
    }

    fn write_to<W: Write>(&self, writer: &mut W, offset_map: &HashMap<u64, u64>) -> io::Result<()> {
        let mut buf = Vec::with_capacity(16);
        buf.write_all(&[TYPE_ROOT])?;
        buf.write_vlq(translate_offset(self.radix_offset, offset_map))?;
        let len = buf.len() + 1;
        buf.write_vlq(len)?;
        writer.write_all(&buf)
    }
}

/// Represent an offset to a dirty entry of a variant type.
/// The lowest 3 bits are used to represnet the actual type.
enum DirtyOffset {
    Radix(usize),
    Leaf(usize),
    Link(usize),
    Key(usize),
}

impl DirtyOffset {
    /// Get type_int for a given dirty_offset.
    #[inline]
    fn peek_type(dirty_offset: u64) -> u8 {
        debug_assert!(dirty_offset >= DIRTY_OFFSET);
        let x = dirty_offset - DIRTY_OFFSET;
        (x & 7) as u8
    }

    /// Get the vec index for a given dirty_offset.
    #[inline]
    fn peek_index(dirty_offset: u64) -> usize {
        debug_assert!(dirty_offset >= DIRTY_OFFSET);
        let x = dirty_offset - DIRTY_OFFSET;
        (x >> 3) as usize
    }
}

impl From<u64> for DirtyOffset {
    fn from(x: u64) -> DirtyOffset {
        debug_assert!(x >= DIRTY_OFFSET);
        let x = x - DIRTY_OFFSET;
        let typeint = (x & 7) as u8;
        let index = (x >> 3) as usize;
        match typeint {
            TYPE_RADIX => DirtyOffset::Radix(index),
            TYPE_LEAF => DirtyOffset::Leaf(index),
            TYPE_LINK => DirtyOffset::Link(index),
            TYPE_KEY => DirtyOffset::Key(index),
            _ => panic!("bug: unexpected dirty offset"),
        }
    }
}

impl Into<u64> for DirtyOffset {
    fn into(self) -> u64 {
        let v = match self {
            DirtyOffset::Radix(x) => (TYPE_RADIX as u64 + ((x as u64) << 3)),
            DirtyOffset::Leaf(x) => (TYPE_LEAF as u64 + ((x as u64) << 3)),
            DirtyOffset::Link(x) => (TYPE_LINK as u64 + ((x as u64) << 3)),
            DirtyOffset::Key(x) => (TYPE_KEY as u64 + ((x as u64) << 3)),
        };
        v + DIRTY_OFFSET
    }
}

/// An Offset to an link entry. This is a standalone type so it cannot be
/// constructed arbitarily.
#[derive(Debug, PartialOrd, PartialEq, Copy, Clone)]
pub struct LinkOffset(u64);

/// Convert a dirty offset to an error. This should be applied to all
/// offsets read from disk - there should never be references to in-memory
/// data.
///
/// With the non_dirty check applied to everywhere, it's safe to assume
/// an offset that is >= DIRTY_OFFSET has a valid content (i.e. it contains
/// a valid vec index).
#[inline]
fn non_dirty(v: io::Result<u64>) -> io::Result<u64> {
    v.and_then(|x| {
        if x >= DIRTY_OFFSET {
            Err(InvalidData.into())
        } else {
            Ok(x)
        }
    })
}

//// Main Index

pub struct Index {
    // For locking and low-level access.
    file: File,

    // For efficient and shared random reading.
    buf: Mmap,

    // Whether "file" was opened as read-only.
    // Only affects "flush". Do not affect in-memory writes.
    read_only: bool,

    // In-memory entries. The root entry is always in-memory.
    root: Root,
    dirty_radixes: Vec<Radix>,
    dirty_leafs: Vec<Leaf>,
    dirty_links: Vec<Link>,
    dirty_keys: Vec<Key>,
}

impl Index {
    /// Open the index file as read-write. Fallback to read-only.
    ///
    /// The index is always writable because it buffers all writes in-memory.
    /// read-only will only cause "flush" to fail.
    ///
    /// If `root_offset` is not 0, read the root entry from the given offset.
    /// Otherwise, read the root entry from the end of the file.
    pub fn open<P: AsRef<Path>>(path: P, root_offset: u64) -> io::Result<Self> {
        let open_result = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .append(true)
            .open(path.as_ref());

        // Fallback to open the file as read-only.
        let (read_only, mut file) = if open_result.is_err() {
            (true, OpenOptions::new().read(true).open(path)?)
        } else {
            (false, open_result.unwrap())
        };

        let (mmap, len) = {
            if root_offset == 0 {
                // Take the lock to read file length, since that decides root entry location.
                let mut lock = ScopedFileLock::new(&mut file, false)?;
                mmap_readonly(lock.as_ref())?
            } else {
                // It's okay to mmap a larger buffer, without locking.
                mmap_readonly(&file)?
            }
        };

        let (dirty_radixes, root) = if root_offset == 0 {
            // Automatically locate the root entry
            if len == 0 {
                // Empty file. Create root radix entry as an dirty entry
                let radix_offset = DirtyOffset::Radix(0).into();
                (vec![Radix::default()], Root { radix_offset })
            } else {
                // Load root entry from the end of file.
                (vec![], Root::read_from_end(&mmap, len)?)
            }
        } else {
            // Load root entry from given offset.
            (vec![], Root::read_from(&mmap, root_offset)?)
        };

        Ok(Index {
            file,
            buf: mmap,
            read_only,
            root,
            dirty_radixes,
            dirty_links: vec![],
            dirty_leafs: vec![],
            dirty_keys: vec![],
        })
    }

    /// Flush dirty parts to disk.
    ///
    /// Return 0 if nothing needs to be written. Otherwise return the
    /// new offset to the root entry.
    ///
    /// Return `PermissionDenied` if the file is read-only.
    pub fn flush(&mut self) -> io::Result<u64> {
        if self.read_only {
            return Err(io::ErrorKind::PermissionDenied.into());
        }

        let mut root_offset = 0;
        if self.root.radix_offset < DIRTY_OFFSET {
            // Nothing changed
            return Ok(root_offset);
        }

        // Critical section: need write lock
        {
            let estimated_dirty_bytes = self.dirty_links.len() * 50;
            let estimated_dirty_offsets = self.dirty_links.len() + self.dirty_keys.len()
                + self.dirty_leafs.len()
                + self.dirty_radixes.len();

            let mut lock = ScopedFileLock::new(&mut self.file, true)?;
            let len = lock.as_mut().seek(SeekFrom::End(0))?;
            let mut buf = Vec::with_capacity(estimated_dirty_bytes);
            let mut offset_map = HashMap::with_capacity(estimated_dirty_offsets);

            // Write in the following order:
            // header, keys, links, leafs, radixes, root.
            // Latter entries depend on former entries.

            if len == 0 {
                buf.write_all(&[TYPE_HEAD])?;
            }

            for (i, entry) in self.dirty_keys.iter().enumerate() {
                let offset = buf.len() as u64 + len;
                entry.write_to(&mut buf, &offset_map)?;
                offset_map.insert(DirtyOffset::Key(i).into(), offset);
            }

            for (i, entry) in self.dirty_links.iter().enumerate() {
                let offset = buf.len() as u64 + len;
                entry.write_to(&mut buf, &offset_map)?;
                offset_map.insert(DirtyOffset::Link(i).into(), offset);
            }

            for (i, entry) in self.dirty_leafs.iter().enumerate() {
                let offset = buf.len() as u64 + len;
                entry.write_to(&mut buf, &offset_map)?;
                offset_map.insert(DirtyOffset::Leaf(i).into(), offset);
            }

            for (i, entry) in self.dirty_radixes.iter().enumerate() {
                let offset = buf.len() as u64 + len;
                entry.write_to(&mut buf, &offset_map)?;
                offset_map.insert(DirtyOffset::Radix(i).into(), offset);
            }

            root_offset = buf.len() as u64 + len;
            self.root.write_to(&mut buf, &offset_map)?;
            lock.as_mut().write_all(&buf)?;

            // Remap and update root since length has changed
            let (mmap, new_len) = mmap_readonly(lock.as_ref())?;
            self.buf = mmap;

            // Sanity check - the length should be expected. Otherwise, the lock
            // is somehow ineffective.
            if new_len != buf.len() as u64 + len {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }

            self.root = Root::read_from_end(&self.buf, new_len)?;
        }

        // Outside critical section
        self.dirty_radixes.clear();
        self.dirty_leafs.clear();
        self.dirty_links.clear();
        self.dirty_keys.clear();

        Ok(root_offset)
    }

    /// Lookup by key. Return the link offset (the head of the linked list), or 0
    /// if the key does not exist. This is a low-level API.
    pub fn get<K: AsRef<[u8]>>(&self, key: &K) -> io::Result<LinkOffset> {
        let mut offset = self.root.radix_offset;
        let mut iter = Base16Iter::from_base256(key);

        while offset != 0 {
            // Read the entry at "offset"
            match self.peek_type(offset)? {
                TYPE_RADIX => {
                    match iter.next() {
                        None => {
                            // The key ends at this Radix entry.
                            return self.peek_radix_entry_link_offset(offset)
                                .map(|v| LinkOffset(v));
                        }
                        Some(x) => {
                            // Should follow the `x`-th child in the Radix entry.
                            offset = self.peek_radix_entry_child(offset, x)?;
                        }
                    }
                }
                TYPE_LEAF => {
                    // Meet a leaf. If key matches, return the LinkOffset.
                    return self.peek_leaf_entry_link_offset_if_matched(offset, key.as_ref())
                        .map(|v| LinkOffset(v));
                }
                _ => return Err(InvalidData.into()),
            }
        }

        // Not found
        Ok(LinkOffset(0))
    }

    /// Insert a new value as a head of the linked list associated with `key`.
    pub fn insert<K: AsRef<[u8]>>(&mut self, key: &K, value: u64) -> io::Result<()> {
        self.insert_advanced(key, value.into(), None)
    }

    /// Update the linked list for a given key.
    ///
    /// - If `value` is not None, `link` is None, a new link entry with
    ///   `value` will be created, and connect to the existing linked
    ///   list pointed by `key`. `key` will point to the new link entry.
    /// - If `value` is None, `link` is not None, `key` will point
    ///   to `link` directly.  This can be used to make multiple
    ///   keys share (part of) a linked list.
    /// - If `value` is not None, and `link` is not None, a new link entry
    ///   with `value` will be created, and connect to `link`. `key` will
    ///   point to the new link entry.
    /// - If `value` and `link` are None. Everything related to `key` is
    ///   marked "dirty" without changing their actual logic value.
    ///
    /// This is a low-level API.
    pub fn insert_advanced<K: AsRef<[u8]>>(
        &mut self,
        key: &K,
        value: Option<u64>,
        link: Option<LinkOffset>,
    ) -> io::Result<()> {
        let mut offset = self.root.radix_offset;
        let mut iter = Base16Iter::from_base256(key);
        let mut step = 0;
        let key = key.as_ref();

        let mut last_radix_offset = 0u64;
        let mut last_child = 0u8;

        loop {
            match self.peek_type(offset)? {
                TYPE_RADIX => {
                    // Copy RadixEntry since we must modify it.
                    offset = self.copy_radix_entry(offset)?;
                    debug_assert!(offset > 0 && self.peek_type(offset)? == TYPE_RADIX);

                    // Change the Root entry, or the previous Radix entry so it
                    // points to the new offset.
                    if step == 0 {
                        self.root.radix_offset = offset;
                    } else {
                        self.set_radix_entry_child(last_radix_offset, last_child, offset);
                    }

                    last_radix_offset = offset;

                    let e = &self.dirty_radixes[DirtyOffset::peek_index(offset)].clone();
                    match iter.next() {
                        None => {
                            let link_offset =
                                self.maybe_create_link_entry(e.link_offset, value, link);
                            self.set_radix_entry_link(offset, link_offset);
                            return Ok(());
                        }
                        Some(x) => {
                            let next_offset = e.offsets[x as usize];
                            if next_offset == 0 {
                                // "key" is longer than existing ones. Create key and leaf entries.
                                let link_offset = self.maybe_create_link_entry(0, value, link);
                                let key_offset = self.create_key_entry(key);
                                let leaf_offset = self.create_leaf_entry(link_offset, key_offset);
                                self.set_radix_entry_child(offset, x, leaf_offset);
                                return Ok(());
                            } else {
                                offset = next_offset;
                                last_child = x;
                            }
                        }
                    }
                }
                TYPE_LEAF => {
                    // TODO: Not implemented yet.
                    let link_offset =
                        self.peek_leaf_entry_link_offset_if_matched(offset, key.as_ref())?;

                    if link_offset > 0 {
                        // Key matched. Need to copy LeafEntry.
                        unimplemented!()
                    } else {
                        // Key mismatch. Do a leaf split.
                        unimplemented!()
                    }
                    return Ok(());
                }
                _ => return Err(InvalidData.into()),
            }

            step += 1;
        }
    }

    /// Return the type_int (TYPE_RADIX, TYPE_LEAF, ...) for a given offset.
    #[inline]
    fn peek_type(&self, offset: u64) -> io::Result<u8> {
        if offset >= DIRTY_OFFSET {
            Ok(DirtyOffset::peek_type(offset))
        } else {
            self.buf
                .get(offset as usize)
                .map(|v| *v)
                .ok_or(InvalidData.into())
        }
    }

    /// Read the link offset from a Radix entry.
    #[inline]
    fn peek_radix_entry_link_offset(&self, offset: u64) -> io::Result<u64> {
        debug_assert_eq!(self.peek_type(offset).unwrap(), TYPE_RADIX);
        if offset >= DIRTY_OFFSET {
            let index = DirtyOffset::peek_index(offset);
            Ok(self.dirty_radixes[index].link_offset)
        } else {
            non_dirty(
                self.buf
                    .read_vlq_at(offset as usize + 1 + 16)
                    .map(|(v, _)| v),
            )
        }
    }

    /// Lookup the `i`-th child inside a Radix entry.
    /// Return stored offset, or 0 if that child does not exist.
    #[inline]
    fn peek_radix_entry_child(&self, offset: u64, i: u8) -> io::Result<u64> {
        debug_assert_eq!(self.peek_type(offset).unwrap(), TYPE_RADIX);
        debug_assert!(i < 16);
        if offset >= DIRTY_OFFSET {
            let index = DirtyOffset::peek_index(offset);
            let e = &self.dirty_radixes[index];
            Ok(e.offsets[i as usize])
        } else {
            // Read from jump table
            match self.buf.get(offset as usize + 1 + i as usize) {
                None => Err(InvalidData.into()),
                Some(&jump) => non_dirty(
                    self.buf
                        .read_vlq_at(offset as usize + jump as usize)
                        .map(|(v, _)| v),
                ),
            }
        }
    }

    /// Return a reference to a Key pointed by a Leaf entry.
    #[inline]
    fn peek_leaf_entry_key(&self, offset: u64) -> io::Result<&[u8]> {
        debug_assert_eq!(self.peek_type(offset).unwrap(), TYPE_LEAF);
        if offset >= DIRTY_OFFSET {
            let index = DirtyOffset::peek_index(offset);
            let leaf = &self.dirty_leafs[index];
            self.peek_key_entry_content(leaf.key_offset)
        } else {
            let (key_offset, vlq_len) = self.buf.read_vlq_at(offset as usize + 1)?;
            non_dirty(Ok(key_offset))?;
            self.peek_key_entry_content(key_offset)
        }
    }

    /// Return the link offset stored in a leaf entry.
    fn peek_leaf_entry_link_offset(&self, offset: u64) -> io::Result<u64> {
        debug_assert_eq!(self.peek_type(offset).unwrap(), TYPE_LEAF);
        if offset >= DIRTY_OFFSET {
            let index = DirtyOffset::peek_index(offset);
            let leaf = &self.dirty_leafs[index];
            Ok(leaf.link_offset)
        } else {
            let (key_offset, vlq_len) = self.buf.read_vlq_at(offset as usize + 1)?;
            non_dirty(Ok(key_offset))?;
            non_dirty(
                self.buf
                    .read_vlq_at(offset as usize + 1 + vlq_len)
                    .map(|(v, _)| v),
            )
        }
    }

    /// Return the link offset stored in a leaf entry if the key matches.
    /// Otherwise return 0.
    fn peek_leaf_entry_link_offset_if_matched(&self, offset: u64, key: &[u8]) -> io::Result<u64> {
        debug_assert_eq!(self.peek_type(offset).unwrap(), TYPE_LEAF);
        if offset >= DIRTY_OFFSET {
            let index = DirtyOffset::peek_index(offset);
            let leaf = &self.dirty_leafs[index];
            if self.check_key_entry_matched(leaf.key_offset, key)? {
                Ok(leaf.link_offset)
            } else {
                Ok(0)
            }
        } else {
            let (key_offset, vlq_len) = self.buf.read_vlq_at(offset as usize + 1)?;
            non_dirty(Ok(key_offset))?;
            if self.check_key_entry_matched(key_offset, key)? {
                non_dirty(
                    self.buf
                        .read_vlq_at(offset as usize + 1 + vlq_len)
                        .map(|(v, _)| v),
                )
            } else {
                Ok(0)
            }
        }
    }

    /// Return a reference to the content of a key entry.
    #[inline]
    fn peek_key_entry_content(&self, offset: u64) -> io::Result<&[u8]> {
        if self.peek_type(offset)? != TYPE_KEY {
            Err(InvalidData.into())
        } else if offset >= DIRTY_OFFSET {
            let index = DirtyOffset::peek_index(offset);
            Ok(&self.dirty_keys[index].key[..])
        } else {
            let (key_len, vlq_len): (usize, _) = self.buf.read_vlq_at(offset as usize + 1)?;
            let start = offset as usize + 1 + vlq_len;
            let end = start + key_len;
            if end > self.buf.len() {
                Err(InvalidData.into())
            } else {
                Ok(&self.buf[start..end])
            }
        }
    }

    /// Return true if the given key matched the key entry.
    #[inline]
    fn check_key_entry_matched(&self, offset: u64, key: &[u8]) -> io::Result<bool> {
        Ok(self.peek_key_entry_content(offset)? == key)
    }

    /// Copy a Radix entry to dirty_radixes. Return its offset.
    /// If the Radix entry is already dirty. Return its offset unchanged.
    #[inline]
    fn copy_radix_entry(&mut self, offset: u64) -> io::Result<u64> {
        if offset < DIRTY_OFFSET {
            let entry = Radix::read_from(&self.buf, offset)?;
            Ok(self.create_radix_entry(entry))
        } else {
            Ok(offset)
        }
    }

    /// Append a Radix entry to dirty_radixes. Return its offset.
    #[inline]
    fn create_radix_entry(&mut self, entry: Radix) -> u64 {
        let index = self.dirty_radixes.len();
        self.dirty_radixes.push(entry);
        DirtyOffset::Radix(index).into()
    }

    /// Set value of a child of a Radix entry.
    #[inline]
    fn set_radix_entry_child(&mut self, radix_offset: u64, i: u8, value: u64) {
        debug_assert!(radix_offset >= DIRTY_OFFSET);
        debug_assert_eq!(DirtyOffset::peek_type(radix_offset), TYPE_RADIX);
        self.dirty_radixes[DirtyOffset::peek_index(radix_offset)].offsets[i as usize] = value;
    }

    /// Set value of the link offset of a Radix entry.
    #[inline]
    fn set_radix_entry_link(&mut self, radix_offset: u64, link_offset: u64) {
        debug_assert!(radix_offset >= DIRTY_OFFSET);
        debug_assert_eq!(DirtyOffset::peek_type(radix_offset), TYPE_RADIX);
        self.dirty_radixes[DirtyOffset::peek_index(radix_offset)].link_offset = link_offset;
    }

    /// See `insert_advanced`. Create a new link entry if necessary and return its offset.
    fn maybe_create_link_entry(
        &mut self,
        link_offset: u64,
        value: Option<u64>,
        link: Option<LinkOffset>,
    ) -> u64 {
        let next_link_offset = link.map_or(link_offset, |v| v.0);
        if let Some(value) = value {
            // Create a new Link entry
            let new_link = Link {
                value,
                next_link_offset,
            };
            let index = self.dirty_links.len();
            self.dirty_links.push(new_link);
            DirtyOffset::Link(index).into()
        } else {
            next_link_offset
        }
    }

    /// Update link_offset of a leaf entry in-place. Copy on write. Return the new leaf_offset
    /// if it's copied from disk.
    ///
    /// Note: the old leaf is expected to be no longer needed. If that's not true, don't call
    /// this function.
    #[inline]
    fn set_leaf_link(&mut self, offset: u64, link_offset: u64) -> io::Result<u64> {
        debug_assert_eq!(DirtyOffset::peek_type(offset), TYPE_LEAF);
        if offset < DIRTY_OFFSET {
            let entry = Leaf::read_from(&self.buf, offset)?;
            Ok(self.create_leaf_entry(link_offset, entry.key_offset))
        } else {
            let index = DirtyOffset::peek_index(offset);
            self.dirty_leafs[index].link_offset = link_offset;
            Ok(offset)
        }
    }

    /// Append a Leaf entry to dirty_leafs. Return its offset.
    #[inline]
    fn create_leaf_entry(&mut self, link_offset: u64, key_offset: u64) -> u64 {
        let index = self.dirty_leafs.len();
        self.dirty_leafs.push(Leaf {
            link_offset,
            key_offset,
        });
        DirtyOffset::Leaf(index).into()
    }

    /// Append a Key entry to dirty_keys. Return its offset.
    #[inline]
    fn create_key_entry(&mut self, key: &[u8]) -> u64 {
        let index = self.dirty_keys.len();
        self.dirty_keys.push(Key {
            key: Vec::from(key),
        });
        DirtyOffset::Key(index).into()
    }
}

//// Debug Formatter

fn fmt_offset(offset: u64, f: &mut Formatter) -> Result<(), fmt::Error> {
    if offset >= DIRTY_OFFSET {
        write!(f, "{:?}", DirtyOffset::from(offset))
    } else if offset == 0 {
        write!(f, "None")
    } else {
        write!(f, "Disk[{}]", offset)
    }
}

impl Debug for DirtyOffset {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        match *self {
            DirtyOffset::Radix(x) => write!(f, "Radix[{}]", x),
            DirtyOffset::Leaf(x) => write!(f, "Leaf[{}]", x),
            DirtyOffset::Link(x) => write!(f, "Link[{}]", x),
            DirtyOffset::Key(x) => write!(f, "Key[{}]", x),
        }
    }
}

impl Debug for Radix {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(f, "Radix {{ link: ")?;
        fmt_offset(self.link_offset, f)?;
        for (i, v) in self.offsets.iter().cloned().enumerate() {
            if v > 0 {
                write!(f, ", {}: ", i)?;
                fmt_offset(v, f)?;
            }
        }
        write!(f, " }}")
    }
}

impl Debug for Leaf {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(f, "Leaf {{ key: ")?;
        fmt_offset(self.key_offset, f)?;
        write!(f, ", link: ")?;
        fmt_offset(self.link_offset, f)?;
        write!(f, " }}")
    }
}

impl Debug for Link {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(f, "Link {{ value: {}, next: ", self.value)?;
        fmt_offset(self.next_link_offset, f)?;
        write!(f, " }}")
    }
}

impl Debug for Key {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(f, "Key {{ key:")?;
        for byte in self.key.iter() {
            write!(f, " {:X}", byte)?;
        }
        write!(f, " }}")
    }
}

impl Debug for Root {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(f, "Root {{ radix: ")?;
        fmt_offset(self.radix_offset, f)?;
        write!(f, " }}")
    }
}

impl Debug for Index {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(f, "Index {{ len: {}, root: ", self.buf.len())?;
        fmt_offset(self.root.radix_offset, f)?;
        write!(f, " }}\n")?;

        // On-disk entries
        let offset_map = HashMap::new();
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
                    let e = Radix::read_from(&self.buf, i).expect("read");
                    e.write_to(&mut buf, &offset_map).expect("write");
                    write!(f, "{:?}\n", e)?;
                }
                TYPE_LEAF => {
                    let e = Leaf::read_from(&self.buf, i).expect("read");
                    e.write_to(&mut buf, &offset_map).expect("write");
                    write!(f, "{:?}\n", e)?;
                }
                TYPE_LINK => {
                    let e = Link::read_from(&self.buf, i).expect("read");
                    e.write_to(&mut buf, &offset_map).expect("write");
                    write!(f, "{:?}\n", e)?;
                }
                TYPE_KEY => {
                    let e = Key::read_from(&self.buf, i).expect("read");
                    e.write_to(&mut buf, &offset_map).expect("write");
                    write!(f, "{:?}\n", e)?;
                }
                TYPE_ROOT => {
                    let e = Root::read_from(&self.buf, i).expect("read");
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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempdir::TempDir;

    quickcheck! {
        fn test_radix_format_roundtrip(v: (u64, u64, u64, u64), link_offset: u64) -> bool {
            let mut offsets = [0; 16];
            offsets[(v.1 + v.2) as usize % 16] = v.0 % DIRTY_OFFSET;
            offsets[(v.0 + v.3) as usize % 16] = v.1 % DIRTY_OFFSET;
            offsets[(v.1 + v.3) as usize % 16] = v.2 % DIRTY_OFFSET;
            offsets[(v.0 + v.2) as usize % 16] = v.3 % DIRTY_OFFSET;

            let radix = Radix { offsets, link_offset };
            let mut buf = vec![1];
            radix.write_to(&mut buf, &HashMap::new()).expect("write");
            let radix1 = Radix::read_from(buf, 1).unwrap();
            radix1 == radix
        }

        fn test_leaf_format_roundtrip(key_offset: u64, link_offset: u64) -> bool {
            let key_offset = key_offset % DIRTY_OFFSET;
            let link_offset = link_offset % DIRTY_OFFSET;
            let leaf = Leaf { key_offset, link_offset };
            let mut buf = vec![1];
            leaf.write_to(&mut buf, &HashMap::new()).expect("write");
            let leaf1 = Leaf::read_from(buf, 1).unwrap();
            leaf1 == leaf
        }

        fn test_link_format_roundtrip(value: u64, next_link_offset: u64) -> bool {
            let next_link_offset = next_link_offset % DIRTY_OFFSET;
            let link = Link { value, next_link_offset };
            let mut buf = vec![1];
            link.write_to(&mut buf, &HashMap::new()).expect("write");
            let link1 = Link::read_from(buf, 1).unwrap();
            link1 == link
        }

        fn test_key_format_roundtrip(key: Vec<u8>) -> bool {
            let entry = Key { key };
            let mut buf = vec![1];
            entry.write_to(&mut buf, &HashMap::new()).expect("write");
            let entry1 = Key::read_from(buf, 1).unwrap();
            entry1 == entry
        }

        fn test_root_format_roundtrip(radix_offset: u64) -> bool {
            let root = Root { radix_offset };
            let mut buf = vec![1];
            root.write_to(&mut buf, &HashMap::new()).expect("write");
            let root1 = Root::read_from(&buf, 1).unwrap();
            let end = buf.len() as u64;
            let root2 = Root::read_from_end(buf, end).unwrap();
            root1 == root && root2 == root
        }
    }

    #[test]
    fn test_distinct_one_byte_keys() {
        let dir = TempDir::new("index").expect("tempdir");
        let mut index = Index::open(dir.path().join("a"), 0).expect("open");
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
            .insert_advanced(&[0x34], 99.into(), link.into())
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
        let dir = TempDir::new("index").expect("tempdir");
        let mut index = Index::open(dir.path().join("a"), 0).expect("open");

        // 1st flush.
        assert_eq!(index.flush().expect("flush"), 19);
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 22, root: Disk[1] }\n\
             Disk[1]: Radix { link: None }\n\
             Disk[19]: Root { radix: Disk[1] }\n"
        );

        // Mixed on-disk and in-memory state.
        index.insert(&[], 55).expect("update");
        index.insert(&[0x12], 77).expect("update");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 22, root: Radix[0] }\n\
             Disk[1]: Radix { link: None }\n\
             Disk[19]: Root { radix: Disk[1] }\n\
             Radix[0]: Radix { link: Link[0], 1: Leaf[0] }\n\
             Leaf[0]: Leaf { key: Key[0], link: Link[1] }\n\
             Link[0]: Link { value: 55, next: None }\n\
             Link[1]: Link { value: 77, next: None }\n\
             Key[0]: Key { key: 12 }\n"
        );

        // After 2nd flush. There are 2 roots.
        let link = index.get(&[0x12]).expect("get");
        index
            .insert_advanced(&[0x34], 99.into(), link.into())
            .expect("update");
        index.flush().expect("flush");
        assert_eq!(
            format!("{:?}", index),
            "Index { len: 66, root: Disk[43] }\n\
             Disk[1]: Radix { link: None }\n\
             Disk[19]: Root { radix: Disk[1] }\n\
             Disk[22]: Key { key: 12 }\n\
             Disk[25]: Key { key: 34 }\n\
             Disk[28]: Link { value: 55, next: None }\n\
             Disk[31]: Link { value: 77, next: None }\n\
             Disk[34]: Link { value: 99, next: Disk[31] }\n\
             Disk[37]: Leaf { key: Disk[22], link: Disk[31] }\n\
             Disk[40]: Leaf { key: Disk[25], link: Disk[34] }\n\
             Disk[43]: Radix { link: Disk[28], 1: Disk[37], 3: Disk[40] }\n\
             Disk[63]: Root { radix: Disk[43] }\n"
        );
    }
}

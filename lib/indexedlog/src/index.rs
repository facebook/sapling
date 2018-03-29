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
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::io::ErrorKind::InvalidData;
use std::path::Path;

use lock::ScopedFileLock;
use utils::mmap_readonly;

use memmap::Mmap;
use vlqencoding::{VLQDecodeAt, VLQEncode};

//// Structures related to file format

#[derive(Clone, PartialEq, Default, Debug)]
struct Radix {
    pub offsets: [u64; 16],
    pub link_offset: u64,
}

#[derive(Clone, PartialEq, Debug)]
struct Leaf {
    pub key_offset: u64,
    pub link_offset: u64,
}

#[derive(Clone, PartialEq, Debug)]
struct Key {
    pub key: Vec<u8>, // base256
}

#[derive(Clone, PartialEq, Debug)]
struct Link {
    pub value: u64,
    pub next_link_offset: u64,
}

#[derive(Clone, PartialEq, Debug)]
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
        buf.write_vlq(self.radix_offset)?;
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

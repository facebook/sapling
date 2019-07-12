// Copyright Facebook, Inc. 2017
//! Trait for serialization and deserialization of tree data.

use crate::errors::*;
use crate::filestate::{FileState, FileStateV2, StateFlags};
use crate::store::BlockId;
use crate::tree::{AggregatedState, Key, Node, NodeEntry, NodeEntryMap};
use crate::treedirstate::TreeDirstateRoot;
use crate::treestate::TreeStateRoot;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use failure::{bail, Fallible};
use std::hash::Hasher;
use std::io::{Cursor, Read, Write};
use twox_hash::XxHash;
use vlqencoding::{VLQDecode, VLQEncode};

pub trait Serializable
where
    Self: Sized,
{
    /// Serialize the storable data to a `Write` stream.
    fn serialize(&self, w: &mut dyn Write) -> Fallible<()>;

    /// Deserialize a new data item from a `Read` stream.
    fn deserialize(r: &mut dyn Read) -> Fallible<Self>;
}

impl Serializable for FileState {
    /// Write a file entry to the store.
    fn serialize(&self, w: &mut dyn Write) -> Fallible<()> {
        w.write_u8(self.state)?;
        w.write_vlq(self.mode)?;
        w.write_vlq(self.size)?;
        w.write_vlq(self.mtime)?;
        Ok(())
    }

    /// Read an entry from the store.
    fn deserialize(r: &mut dyn Read) -> Fallible<FileState> {
        let state = r.read_u8()?;
        let mode = r.read_vlq()?;
        let size = r.read_vlq()?;
        let mtime = r.read_vlq()?;
        Ok(FileState {
            state,
            mode,
            size,
            mtime,
        })
    }
}

impl Serializable for AggregatedState {
    fn serialize(&self, w: &mut dyn Write) -> Fallible<()> {
        w.write_vlq(self.union.to_bits())?;
        w.write_vlq(self.intersection.to_bits())?;
        Ok(())
    }

    /// Read an entry from the store.
    fn deserialize(r: &mut dyn Read) -> Fallible<AggregatedState> {
        let state: u16 = r.read_vlq()?;
        let union = StateFlags::from_bits_truncate(state);
        let state: u16 = r.read_vlq()?;
        let intersection = StateFlags::from_bits_truncate(state);
        Ok(AggregatedState {
            union,
            intersection,
        })
    }
}

impl Serializable for Box<[u8]> {
    fn serialize(&self, w: &mut dyn Write) -> Fallible<()> {
        w.write_vlq(self.len())?;
        w.write_all(&self)?;

        Ok(())
    }

    fn deserialize(r: &mut dyn Read) -> Fallible<Self> {
        let len: usize = r.read_vlq()?;
        let mut buf = vec![0; len];
        r.read_exact(&mut buf)?;
        Ok(buf.into_boxed_slice())
    }
}

impl Serializable for FileStateV2 {
    fn serialize(&self, w: &mut dyn Write) -> Fallible<()> {
        w.write_vlq(self.state.to_bits())?;
        w.write_vlq(self.mode)?;
        w.write_vlq(self.size)?;
        w.write_vlq(self.mtime)?;

        if self.state.contains(StateFlags::COPIED) {
            if let &Some(ref copied) = &self.copied {
                copied.serialize(w)?;
            } else {
                panic!("COPIED flag set without copied path");
            }
        }
        Ok(())
    }

    fn deserialize(r: &mut dyn Read) -> Fallible<FileStateV2> {
        let state: u16 = r.read_vlq()?;
        let state = StateFlags::from_bits_truncate(state);
        let mode = r.read_vlq()?;
        let size = r.read_vlq()?;
        let mtime = r.read_vlq()?;
        let copied = if state.contains(StateFlags::COPIED) {
            Some(Box::<[u8]>::deserialize(r)?)
        } else {
            None
        };

        Ok(FileStateV2 {
            state,
            mode,
            size,
            mtime,
            copied,
        })
    }
}

/// Deserialize a single entry in a node's entry map.  Returns the name and the entry.
fn deserialize_node_entry<T>(r: &mut dyn Read) -> Fallible<(Key, NodeEntry<T>)>
where
    T: Serializable + Clone,
{
    let entry_type = r.read_u8()?;
    match entry_type {
        b'f' => {
            // File entry.
            let data = T::deserialize(r)?;
            let name_len = r.read_vlq()?;
            let mut name = Vec::with_capacity(name_len);
            unsafe {
                // Safe as we've just allocated the buffer and are about to read into it.
                name.set_len(name_len);
            }
            r.read_exact(name.as_mut_slice())?;
            Ok((name.into_boxed_slice(), NodeEntry::File(data)))
        }
        b'd' => {
            // Directory entry.
            let id = r.read_vlq()?;
            let name_len = r.read_vlq()?;
            let mut name = Vec::with_capacity(name_len);
            unsafe {
                // Safe as we've just allocated the buffer and are about to read into it.
                name.set_len(name_len);
            }
            r.read_exact(name.as_mut_slice())?;
            Ok((
                name.into_boxed_slice(),
                NodeEntry::Directory(Node::open(BlockId(id))),
            ))
        }
        _ => {
            bail!(ErrorKind::CorruptTree);
        }
    }
}

impl<T: Serializable + Clone> Serializable for NodeEntryMap<T> {
    fn deserialize(r: &mut dyn Read) -> Fallible<NodeEntryMap<T>> {
        let count = r.read_vlq()?;
        let mut entries = NodeEntryMap::with_capacity(count);
        for _i in 0..count {
            let (name, entry) = deserialize_node_entry(r)?;
            entries.insert_hint_end(name, entry);
        }
        Ok(entries)
    }

    fn serialize(&self, w: &mut dyn Write) -> Fallible<()> {
        w.write_vlq(self.len())?;
        for (name, entry) in self.iter() {
            match entry {
                &NodeEntry::File(ref file) => {
                    w.write_u8(b'f')?;
                    file.serialize(w)?;
                }
                &NodeEntry::Directory(ref node) => {
                    w.write_u8(b'd')?;
                    w.write_vlq(node.id.unwrap().0)?;
                }
            }
            w.write_vlq(name.len())?;
            w.write_all(name)?;
        }
        Ok(())
    }
}

/// Marker indicating that a block is probably a root node.
const DIRSTATE_ROOT_MAGIC_LEN: usize = 4;
const DIRSTATE_ROOT_MAGIC: [u8; DIRSTATE_ROOT_MAGIC_LEN] = *b"////";

impl Serializable for TreeDirstateRoot {
    fn deserialize(r: &mut dyn Read) -> Fallible<TreeDirstateRoot> {
        // Sanity check that this is a root
        let mut buffer = [0; DIRSTATE_ROOT_MAGIC_LEN];
        r.read_exact(&mut buffer)?;
        if buffer != DIRSTATE_ROOT_MAGIC {
            bail!(ErrorKind::CorruptTree);
        }

        let tracked_root_id = BlockId(r.read_u64::<BigEndian>()?);
        let tracked_file_count = r.read_u32::<BigEndian>()?;
        let removed_root_id = BlockId(r.read_u64::<BigEndian>()?);
        let removed_file_count = r.read_u32::<BigEndian>()?;

        Ok(TreeDirstateRoot {
            tracked_root_id,
            tracked_file_count,
            removed_root_id,
            removed_file_count,
        })
    }

    fn serialize(&self, w: &mut dyn Write) -> Fallible<()> {
        w.write_all(&DIRSTATE_ROOT_MAGIC)?;
        w.write_u64::<BigEndian>(self.tracked_root_id.0)?;
        w.write_u32::<BigEndian>(self.tracked_file_count)?;
        w.write_u64::<BigEndian>(self.removed_root_id.0)?;
        w.write_u32::<BigEndian>(self.removed_file_count)?;
        Ok(())
    }
}

#[inline]
fn xxhash<T: AsRef<[u8]>>(buf: T) -> u64 {
    let mut xx = XxHash::default();
    xx.write(buf.as_ref());
    xx.finish()
}

impl Serializable for TreeStateRoot {
    fn deserialize(r: &mut dyn Read) -> Fallible<Self> {
        let checksum = r.read_u64::<BigEndian>()?;
        let mut buf = Vec::new();
        r.read_to_end(&mut buf)?;

        if xxhash(&buf) != checksum {
            bail!(ErrorKind::CorruptTree);
        }

        let mut cur = Cursor::new(buf);
        let version = cur.read_vlq()?;
        if version != 0 {
            bail!(ErrorKind::UnsupportedTreeVersion(version));
        }

        let tree_block_id = BlockId(cur.read_vlq()?);
        let file_count = cur.read_vlq()?;
        let metadata = Box::<[u8]>::deserialize(&mut cur)?;

        Ok(TreeStateRoot {
            version,
            tree_block_id,
            file_count,
            metadata,
        })
    }

    fn serialize(&self, w: &mut dyn Write) -> Fallible<()> {
        let mut buf = Vec::new();
        buf.write_vlq(self.version)?;
        buf.write_vlq(self.tree_block_id.0)?;
        buf.write_vlq(self.file_count)?;
        self.metadata.serialize(&mut buf)?;
        w.write_u64::<BigEndian>(xxhash(&buf))?;
        w.write_all(&buf)?;
        Ok(())
    }
}

impl Serializable for StateFlags {
    fn deserialize(r: &mut dyn Read) -> Fallible<Self> {
        let v = r.read_vlq()?;
        Ok(Self::from_bits_truncate(v))
    }

    fn serialize(&self, w: &mut dyn Write) -> Fallible<()> {
        Ok(w.write_vlq(self.to_bits())?)
    }
}

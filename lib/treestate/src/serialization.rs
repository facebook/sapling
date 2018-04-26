// Copyright Facebook, Inc. 2017
//! Trait for serialization and deserialization of tree data.

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use errors::*;
use filestate::{FileState, StateFlags};
use std::io::{Read, Write};
use store::BlockId;
use tree::{Key, Node, NodeEntry, NodeEntryMap};
use treedirstate::TreeDirstateRoot;
use vlqencoding::{VLQDecode, VLQEncode};

pub trait Serializable
where
    Self: Sized,
{
    /// Serialize the storable data to a `Write` stream.
    fn serialize(&self, w: &mut Write) -> Result<()>;

    /// Deserialize a new data item from a `Read` stream.
    fn deserialize(r: &mut Read) -> Result<Self>;
}

impl Serializable for FileState {
    /// Write a file entry to the store.
    fn serialize(&self, mut w: &mut Write) -> Result<()> {
        w.write_u8(self.state)?;
        w.write_vlq(self.mode)?;
        w.write_vlq(self.size)?;
        w.write_vlq(self.mtime)?;
        Ok(())
    }

    /// Read an entry from the store.
    fn deserialize(mut r: &mut Read) -> Result<FileState> {
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

/// Deserialize a single entry in a node's entry map.  Returns the name and the entry.
fn deserialize_node_entry<T>(mut r: &mut Read) -> Result<(Key, NodeEntry<T>)>
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
            Ok((name, NodeEntry::File(data)))
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
            Ok((name, NodeEntry::Directory(Node::open(BlockId(id)))))
        }
        _ => {
            bail!(ErrorKind::CorruptTree);
        }
    }
}

impl<T: Serializable + Clone> Serializable for NodeEntryMap<T> {
    fn deserialize(r: &mut Read) -> Result<NodeEntryMap<T>> {
        let mut r = r;
        let count = r.read_vlq()?;
        let mut entries = NodeEntryMap::with_capacity(count);
        for _i in 0..count {
            let (name, entry) = deserialize_node_entry(r)?;
            entries.insert_hint_end(name, entry);
        }
        Ok(entries)
    }

    fn serialize(&self, w: &mut Write) -> Result<()> {
        let mut w = w;
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
    fn deserialize(r: &mut Read) -> Result<TreeDirstateRoot> {
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

    fn serialize(&self, w: &mut Write) -> Result<()> {
        w.write_all(&DIRSTATE_ROOT_MAGIC)?;
        w.write_u64::<BigEndian>(self.tracked_root_id.0)?;
        w.write_u32::<BigEndian>(self.tracked_file_count)?;
        w.write_u64::<BigEndian>(self.removed_root_id.0)?;
        w.write_u32::<BigEndian>(self.removed_file_count)?;
        Ok(())
    }
}

impl Serializable for StateFlags {
    fn deserialize(mut r: &mut Read) -> Result<Self> {
        let v = r.read_vlq()?;
        Ok(Self::from_bits_truncate(v))
    }

    fn serialize(&self, mut w: &mut Write) -> Result<()> {
        Ok(w.write_vlq(self.to_bits())?)
    }
}

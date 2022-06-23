/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Trait for serialization and deserialization of tree data.

use std::collections::BTreeMap;
use std::hash::Hasher;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Cursor;
use std::io::Read;
use std::io::Write;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use byteorder::BigEndian;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use twox_hash::XxHash;
use types::hgid::ReadHgIdExt;
use vlqencoding::VLQDecode;
use vlqencoding::VLQEncode;

use crate::dirstate::Dirstate;
use crate::dirstate::TreeStateFields;
use crate::errors::*;
use crate::filestate::FileState;
use crate::filestate::FileStateV2;
use crate::filestate::StateFlags;
use crate::metadata::Metadata;
use crate::store::BlockId;
use crate::tree::AggregatedState;
use crate::tree::Key;
use crate::tree::Node;
use crate::tree::NodeEntry;
use crate::tree::NodeEntryMap;
use crate::treedirstate::TreeDirstateRoot;
use crate::treestate::TreeStateRoot;

pub trait Serializable
where
    Self: Sized,
{
    /// Serialize the storable data to a `Write` stream.
    fn serialize(&self, w: &mut dyn Write) -> Result<()>;

    /// Deserialize a new data item from a `Read` stream.
    fn deserialize(r: &mut dyn Read) -> Result<Self>;
}

impl Serializable for FileState {
    /// Write a file entry to the store.
    fn serialize(&self, w: &mut dyn Write) -> Result<()> {
        w.write_u8(self.state)?;
        w.write_vlq(self.mode)?;
        w.write_vlq(self.size)?;
        w.write_vlq(self.mtime)?;
        Ok(())
    }

    /// Read an entry from the store.
    fn deserialize(r: &mut dyn Read) -> Result<FileState> {
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
    fn serialize(&self, w: &mut dyn Write) -> Result<()> {
        w.write_vlq(self.union.to_bits())?;
        w.write_vlq(self.intersection.to_bits())?;
        Ok(())
    }

    /// Read an entry from the store.
    fn deserialize(r: &mut dyn Read) -> Result<AggregatedState> {
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
    fn serialize(&self, w: &mut dyn Write) -> Result<()> {
        w.write_vlq(self.len())?;
        w.write_all(&self)?;

        Ok(())
    }

    fn deserialize(r: &mut dyn Read) -> Result<Self> {
        let len: usize = r.read_vlq()?;
        let mut buf = vec![0; len];
        r.read_exact(&mut buf)?;
        Ok(buf.into_boxed_slice())
    }
}

impl Serializable for FileStateV2 {
    fn serialize(&self, w: &mut dyn Write) -> Result<()> {
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

    fn deserialize(r: &mut dyn Read) -> Result<FileStateV2> {
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
fn deserialize_node_entry<T>(r: &mut dyn Read) -> Result<(Key, NodeEntry<T>)>
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
    fn deserialize(r: &mut dyn Read) -> Result<NodeEntryMap<T>> {
        let count = r.read_vlq()?;
        let mut entries = NodeEntryMap::with_capacity(count);
        for _i in 0..count {
            let (name, entry) = deserialize_node_entry(r)?;
            entries.insert_hint_end(name, entry);
        }
        Ok(entries)
    }

    fn serialize(&self, w: &mut dyn Write) -> Result<()> {
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
    fn deserialize(r: &mut dyn Read) -> Result<TreeDirstateRoot> {
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

    fn serialize(&self, w: &mut dyn Write) -> Result<()> {
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
    fn deserialize(r: &mut dyn Read) -> Result<Self> {
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

    fn serialize(&self, w: &mut dyn Write) -> Result<()> {
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
    fn deserialize(r: &mut dyn Read) -> Result<Self> {
        let v = r.read_vlq()?;
        Ok(Self::from_bits_truncate(v))
    }

    fn serialize(&self, w: &mut dyn Write) -> Result<()> {
        Ok(w.write_vlq(self.to_bits())?)
    }
}

impl Serializable for Metadata {
    fn serialize(&self, w: &mut dyn Write) -> Result<()> {
        for (i, (k, v)) in self.0.iter().enumerate() {
            if v.is_empty() {
                continue;
            }

            if k.contains(&['=', '\0']) || v.contains('\0') {
                return Err(anyhow!("invalid metadata: {k:?} -> {v:?}"));
            }

            write!(w, "{k}={v}")?;

            if i < self.0.len() - 1 {
                w.write_all(&[0])?;
            }
        }
        Ok(())
    }

    fn deserialize(r: &mut dyn Read) -> Result<Self> {
        let mut buf_reader = BufReader::new(r);
        let mut data = BTreeMap::<String, String>::new();
        loop {
            let mut key = Vec::<u8>::new();
            if buf_reader.read_until(b'=', &mut key)? == 0 {
                break;
            }
            if key.pop() != Some(b'=') {
                return Err(anyhow!("metadata key missing '=': {key:?}"));
            }

            let mut value = Vec::<u8>::new();
            buf_reader.read_until(b'\0', &mut value)?;
            if value.last() == Some(&0) {
                value.pop();
            }

            data.insert(String::from_utf8(key)?, String::from_utf8(value)?);
        }
        Ok(Self(data))
    }
}

const DIRSTATE_TREESTATE_HEADER: &[u8] = b"\ntreestate\n\0";

impl Serializable for Dirstate {
    fn serialize(&self, w: &mut dyn Write) -> Result<()> {
        w.write_all(self.p0.as_ref())?;
        w.write_all(self.p1.as_ref())?;

        let ts_fields = match &self.tree_state {
            Some(ts) => ts,
            None => {
                bail!("tree state fields are required for serializing dirstate")
            }
        };

        w.write_all(DIRSTATE_TREESTATE_HEADER)?;

        let mut meta = Metadata(BTreeMap::from([
            ("filename".to_string(), ts_fields.tree_filename.clone()),
            ("rootid".to_string(), ts_fields.tree_root_id.0.to_string()),
        ]));
        if let Some(threshold) = ts_fields.repack_threshold {
            meta.0
                .insert("threshold".to_string(), threshold.to_string());
        }

        meta.serialize(w)?;

        Ok(())
    }

    /// Best effort parsing of the dirstate. For non-treestate
    /// dirstates we only parse the parents.
    fn deserialize(r: &mut dyn Read) -> Result<Self> {
        let mut ds = Self {
            p0: r.read_hgid()?,
            p1: r.read_hgid()?,
            tree_state: None,
        };

        let mut header_buf = [0; DIRSTATE_TREESTATE_HEADER.len()];
        if r.read_exact(&mut header_buf).is_ok() && header_buf == DIRSTATE_TREESTATE_HEADER {
            let mut meta = Metadata::deserialize(r)?;
            ds.tree_state = Some(TreeStateFields {
                tree_filename: meta
                    .0
                    .remove("filename")
                    .ok_or_else(|| anyhow!("no treestate 'filename' in dirstate"))?,
                tree_root_id: BlockId(
                    meta.0
                        .remove("rootid")
                        .ok_or_else(|| anyhow!("no treestate 'rootid' in dirstate"))?
                        .parse()
                        .context("error parsing dirstate rootid")?,
                ),
                repack_threshold: meta
                    .0
                    .remove("threshold")
                    .map(|t| {
                        t.parse()
                            .with_context(|| format!("error parsing dirstate threshold {:?}", t))
                    })
                    .transpose()?,
            });
        }

        Ok(ds)
    }
}

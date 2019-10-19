// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Classes for constructing and serializing a histpack file and index.
//!
//! A history pack is a pair of files that contain the revision history for
//! various file revisions in Mercurial. It contains only revision history (like
//! parent pointers and linknodes), not any revision content information.
//!
//! It consists of two files, with the following format:
//!
//! ```text
//!
//! .histpack
//!     The pack itself is a series of file revisions with some basic header
//!     information on each.
//!
//!     datapack = <version: 1 byte>
//!                [<filesection>,...]
//!     filesection = <filename len: 2 byte unsigned int>
//!                   <filename>
//!                   <revision count: 4 byte unsigned int>
//!                   [<revision>,...]
//!     revision = <hgid: 20 byte>
//!                <p1node: 20 byte>
//!                <p2node: 20 byte>
//!                <linknode: 20 byte>
//!                <copyfromlen: 2 byte>
//!                <copyfrom>
//!
//!     The revisions within each filesection are stored in topological order
//!     (newest first). If a given entry has a parent from another file (a copy)
//!     then p1node is the hgid from the other file, and copyfrom is the
//!     filepath of the other file.
//!
//! .histidx
//!     The index file provides a mapping from filename to the file section in
//!     the histpack. In V1 it also contains sub-indexes for specific nodes
//!     within each file. It consists of three parts, the fanout, the file index
//!     and the hgid indexes.
//!
//!     The file index is a list of index entries, sorted by filename hash (one
//!     per file section in the pack). Each entry has:
//!
//!     - hgid (The 20 byte hash of the filename)
//!     - pack entry offset (The location of this file section in the histpack)
//!     - pack content size (The on-disk length of this file section's pack
//!                          data)
//!     - hgid index offset (The location of the file's hgid index in the index
//!                          file) [1]
//!     - hgid index size (the on-disk length of this file's hgid index) [1]
//!
//!     The fanout is a quick lookup table to reduce the number of steps for
//!     bisecting the index. It is a series of 4 byte pointers to positions
//!     within the index. It has 2^16 entries, which corresponds to hash
//!     prefixes [00, 01, 02,..., FD, FE, FF]. Example: the pointer in slot 4F
//!     points to the index position of the first revision whose hgid starts
//!     with 4F. This saves log(2^16) bisect steps.
//!
//!     dataidx = <fanouttable>
//!               <file count: 8 byte unsigned> [1]
//!               <fileindex>
//!               <hgid count: 8 byte unsigned> [1]
//!               [<nodeindex>,...] [1]
//!     fanouttable = [<index offset: 4 byte unsigned int>,...] (2^16 entries)
//!
//!     fileindex = [<file index entry>,...]
//!     fileindexentry = <hgid: 20 byte>
//!                      <pack file section offset: 8 byte unsigned int>
//!                      <pack file section size: 8 byte unsigned int>
//!                      <hgid index offset: 4 byte unsigned int> [1]
//!                      <hgid index size: 4 byte unsigned int>   [1]
//!     nodeindex = <filename>[<hgid index entry>,...] [1]
//!     filename = <filename len : 2 byte unsigned int><filename value> [1]
//!     nodeindexentry = <hgid: 20 byte> [1]
//!                      <pack file hgid offset: 8 byte unsigned int> [1]
//!
//! ```
//! [1]: new in version 1.

use std::{
    fs::File,
    io::{Cursor, Read, Write},
    mem::{drop, replace},
    path::{Path, PathBuf},
    sync::Arc,
};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use failure::{format_err, Fail, Fallible};
use memmap::{Mmap, MmapOptions};

use types::{HgId, Key, NodeInfo, RepoPath, RepoPathBuf};
use util::path::remove_file;

use crate::historyindex::HistoryIndex;
use crate::historystore::HistoryStore;
use crate::localstore::LocalStore;
use crate::repack::{Repackable, ToKeys};
use crate::sliceext::SliceExt;

#[derive(Debug, Fail)]
#[fail(display = "Historypack Error: {:?}", _0)]
struct HistoryPackError(String);

#[derive(Clone, Debug, PartialEq)]
pub enum HistoryPackVersion {
    Zero,
    One,
}

impl HistoryPackVersion {
    fn new(value: u8) -> Fallible<Self> {
        match value {
            0 => Ok(HistoryPackVersion::Zero),
            1 => Ok(HistoryPackVersion::One),
            _ => Err(HistoryPackError(format!(
                "invalid history pack version number '{:?}'",
                value
            ))
            .into()),
        }
    }
}

impl From<HistoryPackVersion> for u8 {
    fn from(version: HistoryPackVersion) -> u8 {
        match version {
            HistoryPackVersion::Zero => 0,
            HistoryPackVersion::One => 1,
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct FileSectionHeader<'a> {
    pub file_name: &'a RepoPath,
    pub count: u32,
}

#[derive(Debug, PartialEq)]
pub struct HistoryEntry<'a> {
    pub hgid: HgId,
    pub p1: HgId,
    pub p2: HgId,
    pub link_hgid: HgId,
    pub copy_from: Option<&'a RepoPath>,
}

fn read_slice<'a, 'b>(
    cur: &'a mut Cursor<&[u8]>,
    buf: &'b [u8],
    size: usize,
) -> Fallible<&'b [u8]> {
    let start = cur.position() as usize;
    let end = start + size;
    let file_name = buf.get_err(start..end)?;
    cur.set_position(end as u64);
    Ok(file_name)
}

impl<'a> FileSectionHeader<'a> {
    pub(crate) fn read(buf: &[u8]) -> Fallible<FileSectionHeader> {
        let mut cur = Cursor::new(buf);
        let file_name_len = cur.read_u16::<BigEndian>()? as usize;
        let file_name_slice = read_slice(&mut cur, &buf, file_name_len)?;
        let file_name = RepoPath::from_utf8(file_name_slice)?;

        let count = cur.read_u32::<BigEndian>()?;
        Ok(FileSectionHeader { file_name, count })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Fallible<()> {
        let file_name_slice = self.file_name.as_byte_slice();
        writer.write_u16::<BigEndian>(file_name_slice.len() as u16)?;
        writer.write_all(file_name_slice)?;
        writer.write_u32::<BigEndian>(self.count)?;
        Ok(())
    }
}

impl<'a> HistoryEntry<'a> {
    pub(crate) fn read(buf: &[u8]) -> Fallible<HistoryEntry> {
        let mut cur = Cursor::new(buf);
        let mut hgid_buf: [u8; 20] = Default::default();

        // Node
        cur.read_exact(&mut hgid_buf)?;
        let hgid = HgId::from(&hgid_buf);

        // Parents
        cur.read_exact(&mut hgid_buf)?;
        let p1 = HgId::from(&hgid_buf);
        cur.read_exact(&mut hgid_buf)?;
        let p2 = HgId::from(&hgid_buf);

        // LinkNode
        cur.read_exact(&mut hgid_buf)?;
        let link_hgid = HgId::from(&hgid_buf);

        // Copyfrom
        let copy_from_len = cur.read_u16::<BigEndian>()? as usize;
        let copy_from = if copy_from_len > 0 {
            let slice = read_slice(&mut cur, &buf, copy_from_len)?;
            Some(RepoPath::from_utf8(slice)?)
        } else {
            None
        };

        Ok(HistoryEntry {
            hgid,
            p1,
            p2,
            link_hgid,
            copy_from,
        })
    }

    pub fn write<T: Write>(
        writer: &mut T,
        hgid: &HgId,
        p1: &HgId,
        p2: &HgId,
        linknode: &HgId,
        copy_from: &Option<&RepoPath>,
    ) -> Fallible<()> {
        writer.write_all(hgid.as_ref())?;
        writer.write_all(p1.as_ref())?;
        writer.write_all(p2.as_ref())?;
        writer.write_all(linknode.as_ref())?;
        match copy_from {
            &Some(file_name) => {
                let file_name_slice = file_name.as_byte_slice();
                writer.write_u16::<BigEndian>(file_name_slice.len() as u16)?;
                writer.write_all(file_name_slice)?;
            }
            &None => writer.write_u16::<BigEndian>(0)?,
        };

        Ok(())
    }
}

pub struct HistoryPack {
    mmap: Mmap,
    #[allow(dead_code)]
    version: HistoryPackVersion,
    index: HistoryIndex,
    base_path: Arc<PathBuf>,
    pack_path: PathBuf,
    index_path: PathBuf,
}

impl HistoryPack {
    pub fn new(path: &Path) -> Fallible<Self> {
        let base_path = PathBuf::from(path);
        let pack_path = path.with_extension("histpack");
        let file = File::open(&pack_path)?;
        let len = file.metadata()?.len();
        if len < 1 {
            return Err(format_err!(
                "empty histpack '{:?}' is invalid",
                path.to_str().unwrap_or("<unknown>")
            ));
        }

        let mmap = unsafe { MmapOptions::new().len(len as usize).map(&file)? };
        let version = HistoryPackVersion::new(mmap[0])?;
        if version != HistoryPackVersion::One {
            return Err(HistoryPackError(format!("version {:?} not supported", version)).into());
        }

        let index_path = path.with_extension("histidx");
        Ok(HistoryPack {
            mmap,
            version,
            index: HistoryIndex::new(&index_path)?,
            base_path: Arc::new(base_path),
            pack_path,
            index_path,
        })
    }

    pub fn len(&self) -> usize {
        self.mmap.len()
    }

    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    pub fn pack_path(&self) -> &Path {
        &self.pack_path
    }

    pub fn index_path(&self) -> &Path {
        &self.index_path
    }

    fn read_file_section_header(&self, offset: u64) -> Fallible<FileSectionHeader> {
        FileSectionHeader::read(&self.mmap.as_ref().get_err(offset as usize..)?)
    }

    fn read_history_entry(&self, offset: u64) -> Fallible<HistoryEntry> {
        HistoryEntry::read(&self.mmap.as_ref().get_err(offset as usize..)?)
    }

    fn read_node_info(&self, key: &Key, offset: u64) -> Fallible<NodeInfo> {
        let entry = self.read_history_entry(offset)?;
        assert_eq!(entry.hgid, key.hgid);
        let p1 = Key::new(
            match entry.copy_from {
                Some(value) => value.to_owned(),
                None => key.path.clone(),
            },
            entry.p1.clone(),
        );
        let p2 = Key::new(key.path.clone(), entry.p2.clone());

        Ok(NodeInfo {
            parents: [p1, p2],
            linknode: entry.link_hgid.clone(),
        })
    }
}

impl HistoryStore for HistoryPack {
    fn get_node_info(&self, key: &Key) -> Fallible<Option<NodeInfo>> {
        let hgid_location = match self.index.get_hgid_entry(key)? {
            None => return Ok(None),
            Some(location) => location,
        };
        self.read_node_info(key, hgid_location.offset).map(Some)
    }
}

impl LocalStore for HistoryPack {
    fn from_path(path: &Path) -> Fallible<Self> {
        HistoryPack::new(path)
    }

    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        Ok(keys
            .iter()
            .filter(|k| match self.index.get_hgid_entry(&k) {
                Ok(None) | Err(_) => true,
                Ok(Some(_)) => false,
            })
            .map(|k| k.clone())
            .collect())
    }
}

impl ToKeys for HistoryPack {
    fn to_keys(&self) -> Vec<Fallible<Key>> {
        HistoryPackIterator::new(self).collect()
    }
}

impl Repackable for HistoryPack {
    fn delete(mut self) -> Fallible<()> {
        // On some platforms, removing a file can fail if it's still opened or mapped, let's make
        // sure we close and unmap them before deletion.
        let pack_path = replace(&mut self.pack_path, Default::default());
        let index_path = replace(&mut self.index_path, Default::default());
        drop(self);

        let result1 = remove_file(&pack_path);
        let result2 = remove_file(&index_path);
        // Only check for errors after both have run. That way if pack_path doesn't exist,
        // index_path is still deleted.
        result1?;
        result2?;
        Ok(())
    }
}

struct HistoryPackIterator<'a> {
    pack: &'a HistoryPack,
    offset: u64,
    current_name: RepoPathBuf,
    current_remaining: u32,
}

impl<'a> HistoryPackIterator<'a> {
    pub fn new(pack: &'a HistoryPack) -> Self {
        HistoryPackIterator {
            pack,
            offset: 1, // Start after the header byte
            current_name: RepoPathBuf::new(),
            current_remaining: 0,
        }
    }
}

impl<'a> Iterator for HistoryPackIterator<'a> {
    type Item = Fallible<Key>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.current_remaining == 0 && (self.offset as usize) < self.pack.len() {
            let file_header = self.pack.read_file_section_header(self.offset);
            match file_header {
                Ok(header) => {
                    let file_name_slice = header.file_name.as_byte_slice();
                    self.current_name = header.file_name.to_owned();
                    self.current_remaining = header.count;
                    self.offset += 4 + 2 + file_name_slice.len() as u64;
                }
                Err(e) => {
                    self.offset = self.pack.len() as u64;
                    return Some(Err(e));
                }
            };
        }

        if self.offset as usize >= self.pack.len() {
            return None;
        }

        let entry = self.pack.read_history_entry(self.offset);
        self.current_remaining -= 1;
        Some(match entry {
            Ok(ref e) => {
                self.offset += 80;
                self.offset += match e.copy_from {
                    Some(path) => 2 + path.as_byte_slice().len() as u64,
                    None => 2,
                };
                Ok(Key::new(self.current_name.clone(), e.hgid))
            }
            Err(e) => {
                // The entry is corrupted, and we have no way to know where the next one is
                // located, let's forcibly stop the iteration.
                self.offset = self.pack.len() as u64;
                Err(e)
            }
        })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    use quickcheck::quickcheck;
    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;
    use tempfile::TempDir;

    use std::{
        collections::HashMap,
        fs::{set_permissions, File, OpenOptions},
    };

    use types::{testutil::*, RepoPathBuf};

    use crate::{historystore::MutableHistoryStore, mutablehistorypack::MutableHistoryPack};

    pub fn make_historypack(tempdir: &TempDir, nodes: &HashMap<Key, NodeInfo>) -> HistoryPack {
        let mutpack = MutableHistoryPack::new(tempdir.path(), HistoryPackVersion::One).unwrap();
        for (ref key, ref info) in nodes.iter() {
            mutpack.add(key.clone(), info.clone()).unwrap();
        }

        let path = mutpack.flush().unwrap().unwrap();

        HistoryPack::new(&path).unwrap()
    }

    pub fn get_nodes(mut rng: &mut ChaChaRng) -> HashMap<Key, NodeInfo> {
        let file1 = RepoPath::from_str("path").unwrap();
        let file2 = RepoPath::from_str("path/file").unwrap();
        let null = HgId::null_id();
        let node1 = HgId::random(&mut rng);
        let node2 = HgId::random(&mut rng);
        let node3 = HgId::random(&mut rng);
        let node4 = HgId::random(&mut rng);
        let node5 = HgId::random(&mut rng);
        let node6 = HgId::random(&mut rng);

        let mut nodes = HashMap::new();

        // Insert key 1
        let key1 = Key::new(file1.to_owned(), node2.clone());
        let info = NodeInfo {
            parents: [
                Key::new(file1.to_owned(), node1.clone()),
                Key::new(file1.to_owned(), null.clone()),
            ],
            linknode: HgId::random(&mut rng),
        };
        nodes.insert(key1.clone(), info.clone());

        // Insert key 2
        let key2 = Key::new(file2.to_owned(), node3.clone());
        let info = NodeInfo {
            parents: [
                Key::new(file2.to_owned(), node5.clone()),
                Key::new(file2.to_owned(), node6.clone()),
            ],
            linknode: HgId::random(&mut rng),
        };
        nodes.insert(key2.clone(), info.clone());

        // Insert key 3
        let key3 = Key::new(file1.to_owned(), node4.clone());
        let info = NodeInfo {
            parents: [key2.clone(), key1.clone()],
            linknode: HgId::random(&mut rng),
        };
        nodes.insert(key3.clone(), info.clone());

        nodes
    }

    #[test]
    fn test_get_node_info() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new().unwrap();

        let nodes = get_nodes(&mut rng);

        let pack = make_historypack(&tempdir, &nodes);

        for (ref key, ref info) in nodes.iter() {
            let response: NodeInfo = pack.get_node_info(key).unwrap().unwrap();
            assert_eq!(response, **info);
        }
    }

    #[test]
    fn test_get_missing() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new().unwrap();

        let nodes = get_nodes(&mut rng);

        let pack = make_historypack(&tempdir, &nodes);

        let mut test_keys: Vec<Key> = nodes.keys().map(|k| k.clone()).collect();
        let missing_key = key("missing", "f0f0f0");
        test_keys.push(missing_key.clone());

        let missing = pack.get_missing(&test_keys[..]).unwrap();
        assert_eq!(vec![missing_key], missing);
    }

    #[test]
    fn test_iter() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new().unwrap();

        let nodes = get_nodes(&mut rng);

        let pack = make_historypack(&tempdir, &nodes);

        let mut keys: Vec<Key> = nodes.keys().map(|k| k.clone()).collect();
        keys.sort_unstable();
        let mut iter_keys = pack
            .to_keys()
            .into_iter()
            .collect::<Fallible<Vec<Key>>>()
            .unwrap();
        iter_keys.sort_unstable();
        assert_eq!(iter_keys, keys,);
    }

    #[test]
    fn test_open_v0() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new().unwrap();

        let nodes = get_nodes(&mut rng);

        let mutpack = MutableHistoryPack::new(tempdir.path(), HistoryPackVersion::One).unwrap();
        for (ref key, ref info) in nodes.iter() {
            mutpack.add(key.clone(), info.clone()).unwrap();
        }

        let path = mutpack.flush().unwrap().unwrap();
        let pack_path = path.with_extension("histpack");

        let mut buf = Vec::new();
        {
            let mut file = File::open(&pack_path).unwrap();
            file.read_to_end(&mut buf).unwrap();

            // After being closed the datapacks are read-only. Since the next part of the test
            // corrupt it, let's make it writable.
            let mut perms = file.metadata().unwrap().permissions();
            perms.set_readonly(false);

            drop(file);
            set_permissions(&pack_path, perms).unwrap();
        }
        buf[0] = 0;
        OpenOptions::new()
            .write(true)
            .open(&pack_path)
            .unwrap()
            .write_all(&buf)
            .unwrap();

        assert!(HistoryPack::new(&pack_path).is_err());
    }

    quickcheck! {
        fn test_file_section_header_serialization(path: RepoPathBuf, count: u32) -> bool {
            let header = FileSectionHeader {
                file_name: path.as_ref(),
                count: count,
            };
            let mut buf = vec![];
            header.write(&mut buf).unwrap();
            header == FileSectionHeader::read(&buf).unwrap()
        }

        fn test_history_entry_serialization(
            hgid: HgId,
            p1: HgId,
            p2: HgId,
            link_hgid: HgId,
            copy_from: Option<RepoPathBuf>
        ) -> bool {
            let mut buf = vec![];
            HistoryEntry::write(
                &mut buf,
                &hgid,
                &p1,
                &p2,
                &link_hgid,
                &copy_from.as_ref().map(|x| x.as_ref()),
            ).unwrap();
            let entry = HistoryEntry::read(&buf).unwrap();
            assert_eq!(hgid, entry.hgid);
            assert_eq!(p1, entry.p1);
            assert_eq!(p2, entry.p2);
            assert_eq!(link_hgid, entry.link_hgid);
            true
        }
    }
}

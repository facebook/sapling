// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fs::File,
    io::{Cursor, Read, Write},
    path::Path,
};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use crypto::digest::Digest;
use crypto::sha1::Sha1;
use failure::{Fail, Fallible};
use memmap::{Mmap, MmapOptions};

use types::{HgId, Key, RepoPath};

use crate::fanouttable::FanoutTable;
use crate::historypack::HistoryPackVersion;
use crate::sliceext::SliceExt;

#[derive(Debug, Fail)]
#[fail(display = "HistoryIndex Error: {:?}", _0)]
struct HistoryIndexError(String);

const SMALL_FANOUT_CUTOFF: usize = 8192; // 2^16 / 8

#[derive(Debug, PartialEq)]
struct HistoryIndexOptions {
    pub version: HistoryPackVersion,
    // Indicates whether to use the large fanout (2 bytes) or the small (1 byte)
    pub large: bool,
}

impl HistoryIndexOptions {
    pub fn read<T: Read>(reader: &mut T) -> Fallible<HistoryIndexOptions> {
        let version = reader.read_u8()?;
        let version = match version {
            0 => HistoryPackVersion::Zero,
            1 => HistoryPackVersion::One,
            _ => {
                return Err(
                    HistoryIndexError(format!("unsupported version '{:?}'", version)).into(),
                );
            }
        };

        let raw_config = reader.read_u8()?;
        let large = match raw_config {
            0b10000000 => true,
            0 => false,
            _ => {
                return Err(
                    HistoryIndexError(format!("invalid history index '{:?}'", raw_config)).into(),
                );
            }
        };
        Ok(HistoryIndexOptions { version, large })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Fallible<()> {
        writer.write_u8(match self.version {
            HistoryPackVersion::Zero => 0,
            HistoryPackVersion::One => 1,
        })?;
        writer.write_u8(if self.large { 0b10000000 } else { 0 })?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct FileSectionLocation {
    pub offset: u64,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct NodeLocation {
    pub offset: u64,
}

#[derive(PartialEq, Debug)]
pub(crate) struct FileIndexEntry {
    pub hgid: HgId,
    pub file_section_offset: u64,
    pub file_section_size: u64,
    pub hgid_index_offset: u32,
    pub hgid_index_size: u32,
}
const FILE_ENTRY_LEN: usize = 44;

impl FileIndexEntry {
    pub fn read(buf: &[u8]) -> Fallible<Self> {
        let mut cur = Cursor::new(buf);
        cur.set_position(20);
        let hgid_slice: &[u8] = buf.get_err(0..20)?;
        Ok(FileIndexEntry {
            hgid: HgId::from_slice(hgid_slice)?,
            file_section_offset: cur.read_u64::<BigEndian>()?,
            file_section_size: cur.read_u64::<BigEndian>()?,
            hgid_index_offset: cur.read_u32::<BigEndian>()?,
            hgid_index_size: cur.read_u32::<BigEndian>()?,
        })
    }

    fn write<T: Write>(&self, writer: &mut T) -> Fallible<()> {
        writer.write_all(self.hgid.as_ref())?;
        writer.write_u64::<BigEndian>(self.file_section_offset)?;
        writer.write_u64::<BigEndian>(self.file_section_size)?;
        writer.write_u32::<BigEndian>(self.hgid_index_offset)?;
        writer.write_u32::<BigEndian>(self.hgid_index_size)?;
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct NodeIndexEntry {
    pub hgid: HgId,
    pub offset: u64,
}
const NODE_ENTRY_LEN: usize = 28;

impl NodeIndexEntry {
    pub fn read(buf: &[u8]) -> Fallible<Self> {
        let mut cur = Cursor::new(buf);
        cur.set_position(20);
        let hgid_slice: &[u8] = buf.get_err(0..20)?;
        Ok(NodeIndexEntry {
            hgid: HgId::from_slice(hgid_slice)?,
            offset: cur.read_u64::<BigEndian>()?,
        })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Fallible<()> {
        writer.write_all(self.hgid.as_ref())?;
        writer.write_u64::<BigEndian>(self.offset)?;
        Ok(())
    }
}

pub(crate) struct HistoryIndex {
    mmap: Mmap,
    #[allow(dead_code)]
    version: HistoryPackVersion,
    fanout_size: usize,
    index_start: usize,
    index_end: usize,
}

impl HistoryIndex {
    pub fn new(path: &Path) -> Fallible<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        if len < 1 {
            return Err(HistoryIndexError(format!(
                "empty histidx '{:?}' is invalid",
                path.to_str().unwrap_or("<unknown>")
            ))
            .into());
        }

        let mmap = unsafe { MmapOptions::new().len(len as usize).map(&file)? };
        let options = HistoryIndexOptions::read(&mut Cursor::new(&mmap))?;
        let version = options.version;
        let fanout_size = FanoutTable::get_size(options.large);
        let mut index_start = 2 + fanout_size;

        let mut index_end = mmap.len();
        // Version one records the number of entries in the index
        if version == HistoryPackVersion::One {
            let mut cursor = Cursor::new(&mmap);
            cursor.set_position(index_start as u64);
            let file_count = cursor.read_u64::<BigEndian>()? as usize;
            index_start += 8;
            index_end = index_start + (file_count * FILE_ENTRY_LEN);
        }

        Ok(HistoryIndex {
            mmap,
            version,
            fanout_size,
            index_start,
            index_end,
        })
    }

    pub fn write<T: Write>(
        writer: &mut T,
        file_sections: &[(&RepoPath, FileSectionLocation)],
        nodes: &HashMap<&RepoPath, HashMap<Key, NodeLocation>>,
    ) -> Fallible<()> {
        // Write header
        let options = HistoryIndexOptions {
            version: HistoryPackVersion::One,
            large: file_sections.len() > SMALL_FANOUT_CUTOFF,
        };
        options.write(writer)?;

        let mut file_sections: Vec<(&RepoPath, HgId, FileSectionLocation)> = file_sections
            .iter()
            .map(|e| Ok((e.0, sha1(&e.0.as_byte_slice()), e.1.clone())))
            .collect::<Fallible<Vec<(&RepoPath, HgId, FileSectionLocation)>>>()?;
        // They must be written in sorted order so they can be bisected.
        file_sections.sort_by_key(|x| x.1);

        // Write the fanout table
        FanoutTable::write(
            writer,
            if options.large { 2 } else { 1 },
            &mut file_sections.iter().map(|x| &x.1),
            FILE_ENTRY_LEN,
            None,
        )?;

        // Write out the number of files in the file portion.
        writer.write_u64::<BigEndian>(file_sections.len() as u64)?;

        <HistoryIndex>::write_file_index(writer, &options, &file_sections, nodes)?;

        // For each file, write a hgid index
        for &(file_name, ..) in file_sections.iter() {
            <HistoryIndex>::write_hgid_section(writer, nodes, file_name)?;
        }

        Ok(())
    }

    fn write_file_index<T: Write>(
        writer: &mut T,
        options: &HistoryIndexOptions,
        file_sections: &Vec<(&RepoPath, HgId, FileSectionLocation)>,
        nodes: &HashMap<&RepoPath, HashMap<Key, NodeLocation>>,
    ) -> Fallible<()> {
        // For each file, keep track of where its hgid index will start.
        // The first ones starts after the header, fanout, file count, file section, and hgid count.
        let mut hgid_offset: usize = 2
            + FanoutTable::get_size(options.large)
            + 8
            + (file_sections.len() * FILE_ENTRY_LEN)
            + 8;
        let mut hgid_count = 0;

        // Write out the file section entries
        let mut seen_files = HashSet::<&RepoPath>::with_capacity(file_sections.len());
        for &(file_name, file_hash, ref section_location) in file_sections.iter() {
            if seen_files.contains(file_name) {
                return Err(HistoryIndexError(format!(
                    "file '{:?}' was specified twice",
                    file_name
                ))
                .into());
            }
            seen_files.insert(&file_name);

            let file_nodes: &HashMap<Key, NodeLocation> =
                nodes.get(file_name).ok_or_else(|| {
                    HistoryIndexError(format!("unable to find nodes for {:?}", file_name))
                })?;
            let hgid_section_size = file_nodes.len() * NODE_ENTRY_LEN;
            FileIndexEntry {
                hgid: file_hash.clone(),
                file_section_offset: section_location.offset,
                file_section_size: section_location.size,
                hgid_index_offset: hgid_offset as u32,
                hgid_index_size: hgid_section_size as u32,
            }
            .write(writer)?;

            // Keep track of the current hgid index offset
            hgid_offset += 2 + file_name.as_byte_slice().len() + hgid_section_size;
            hgid_count += file_nodes.len();
        }

        // Write the total number of nodes
        writer.write_u64::<BigEndian>(hgid_count as u64)?;

        Ok(())
    }

    fn write_hgid_section<T: Write>(
        writer: &mut T,
        nodes: &HashMap<&RepoPath, HashMap<Key, NodeLocation>>,
        file_name: &RepoPath,
    ) -> Fallible<()> {
        // Write the filename
        let file_name_slice = file_name.as_byte_slice();
        writer.write_u16::<BigEndian>(file_name_slice.len() as u16)?;
        writer.write_all(file_name_slice)?;

        // Write each hgid, in sorted order so the can be bisected
        let file_nodes = nodes.get(file_name).ok_or_else(|| {
            HistoryIndexError(format!("unabled to find nodes for {:?}", file_name))
        })?;
        let mut file_nodes: Vec<(&Key, &NodeLocation)> =
            file_nodes.iter().collect::<Vec<(&Key, &NodeLocation)>>();
        file_nodes.sort_by_key(|x| x.0.hgid);

        for &(key, location) in file_nodes.iter() {
            NodeIndexEntry {
                hgid: key.hgid.clone(),
                offset: location.offset,
            }
            .write(writer)?;
        }

        Ok(())
    }

    pub fn get_file_entry(&self, key: &Key) -> Fallible<Option<FileIndexEntry>> {
        let filename_hgid = sha1(key.path.as_byte_slice());
        let (start, end) = FanoutTable::get_bounds(self.get_fanout_slice(), &filename_hgid)?;
        let start = start + self.index_start;
        let end = end
            .map(|pos| pos + self.index_start)
            .unwrap_or(self.index_end);

        let buf = self.mmap.get_err(start..end)?;
        let entry_offset = match self.binary_search_files(&filename_hgid, buf) {
            None => return Ok(None),
            Some(offset) => offset,
        };
        self.read_file_entry((start + entry_offset) - self.index_start)
            .map(Some)
    }

    pub fn get_hgid_entry(&self, key: &Key) -> Fallible<Option<NodeIndexEntry>> {
        let file_entry = match self.get_file_entry(&key)? {
            None => return Ok(None),
            Some(entry) => entry,
        };

        let start = file_entry.hgid_index_offset as usize + 2 + key.path.as_byte_slice().len();
        let end = start + file_entry.hgid_index_size as usize;

        let buf = self.mmap.get_err(start..end)?;
        let entry_offset = match self.binary_search_nodes(&key.hgid, &buf) {
            None => return Ok(None),
            Some(offset) => offset,
        };

        self.read_hgid_entry((start + entry_offset) - self.index_start)
            .map(Some)
    }

    fn read_file_entry(&self, offset: usize) -> Fallible<FileIndexEntry> {
        FileIndexEntry::read(self.read_data(offset, FILE_ENTRY_LEN)?)
    }

    fn read_hgid_entry(&self, offset: usize) -> Fallible<NodeIndexEntry> {
        NodeIndexEntry::read(self.read_data(offset, NODE_ENTRY_LEN)?)
    }

    fn read_data(&self, offset: usize, size: usize) -> Fallible<&[u8]> {
        let offset = offset + self.index_start;
        Ok(self.mmap.get_err(offset..offset + size)?)
    }

    // These two binary_search_* functions are very similar, but I couldn't find a way to unify
    // them without using macros.
    fn binary_search_files(&self, key: &HgId, slice: &[u8]) -> Option<usize> {
        let size = slice.len() / FILE_ENTRY_LEN;
        // Cast the slice into an array of entry buffers so we can bisect across them
        let slice: &[[u8; FILE_ENTRY_LEN]] = unsafe {
            ::std::slice::from_raw_parts(slice.as_ptr() as *const [u8; FILE_ENTRY_LEN], size)
        };
        let search_result = slice.binary_search_by(|entry| match entry.get(0..20) {
            Some(buf) => buf.cmp(key.as_ref()),
            None => Ordering::Greater,
        });
        match search_result {
            Ok(offset) => Some(offset * FILE_ENTRY_LEN),
            Err(_offset) => None,
        }
    }

    fn binary_search_nodes(&self, key: &HgId, slice: &[u8]) -> Option<usize> {
        let size = slice.len() / NODE_ENTRY_LEN;
        // Cast the slice into an array of entry buffers so we can bisect across them
        let slice: &[[u8; NODE_ENTRY_LEN]] = unsafe {
            ::std::slice::from_raw_parts(slice.as_ptr() as *const [u8; NODE_ENTRY_LEN], size)
        };
        let search_result = slice.binary_search_by(|entry| match entry.get(0..20) {
            Some(buf) => buf.cmp(key.as_ref()),
            None => Ordering::Greater,
        });
        match search_result {
            Ok(offset) => Some(offset * NODE_ENTRY_LEN),
            Err(_offset) => None,
        }
    }

    fn get_fanout_slice(&self) -> &[u8] {
        self.mmap[2..2 + self.fanout_size].as_ref()
    }
}

fn sha1(value: &[u8]) -> HgId {
    let mut hasher = Sha1::new();
    hasher.input(value);
    let mut buf: [u8; 20] = Default::default();
    hasher.result(&mut buf);
    (&buf).into()
}

#[cfg(test)]
impl quickcheck::Arbitrary for FileSectionLocation {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        FileSectionLocation {
            offset: g.next_u64(),
            size: g.next_u64(),
        }
    }
}

#[cfg(test)]
impl quickcheck::Arbitrary for NodeLocation {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        NodeLocation {
            offset: g.next_u64(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use quickcheck::quickcheck;
    use tempfile::NamedTempFile;

    use types::path::RepoPathBuf;

    fn make_index(
        file_sections: &[(&RepoPath, FileSectionLocation)],
        nodes: &HashMap<&RepoPath, HashMap<Key, NodeLocation>>,
    ) -> HistoryIndex {
        let mut file = NamedTempFile::new().unwrap();
        HistoryIndex::write(&mut file, file_sections, nodes).unwrap();
        let path = file.into_temp_path();

        HistoryIndex::new(&path).unwrap()
    }

    quickcheck! {
        fn test_file_index_entry_roundtrip(
            hgid: HgId,
            file_section_offset: u64,
            file_section_size: u64,
            hgid_index_offset: u32,
            hgid_index_size: u32
        ) -> bool {
            let entry = FileIndexEntry {
                hgid,
                file_section_offset,
                file_section_size,
                hgid_index_offset,
                hgid_index_size,
            };

            let mut buf: Vec<u8> = vec![];
            entry.write(&mut buf).unwrap();
            entry == FileIndexEntry::read(buf.as_ref()).unwrap()
        }

        fn test_hgid_index_entry_roundtrip(hgid: HgId, offset: u64) -> bool {
            let entry = NodeIndexEntry {
                hgid, offset
            };

            let mut buf: Vec<u8> = vec![];
            entry.write(&mut buf).unwrap();
            entry == NodeIndexEntry::read(buf.as_ref()).unwrap()
        }

        fn test_options_serialization(version: u8, large: bool) -> bool {
            let version = if version % 2 == 0 { HistoryPackVersion::Zero } else { HistoryPackVersion::One };
            let options = HistoryIndexOptions { version, large };
            let mut buf: Vec<u8> = vec![];
            options.write(&mut buf).expect("write");
            let parsed_options = HistoryIndexOptions::read(&mut Cursor::new(buf)).expect("read");
            options == parsed_options
        }

        fn test_roundtrip_index(data: Vec<(RepoPathBuf, (FileSectionLocation, HashMap<Key, NodeLocation>))>) -> bool {
            let mut file_sections: Vec<(&RepoPath, FileSectionLocation)> = vec![];
            let mut file_nodes: HashMap<&RepoPath, HashMap<Key, NodeLocation>> = HashMap::new();

            let mut seen_files: HashSet<RepoPathBuf> = HashSet::new();
            for &(ref path, (ref location, ref nodes)) in data.iter() {
                // Don't allow a filename to be used twice
                if seen_files.contains(path) {
                    continue;
                }
                seen_files.insert(path.clone());

                file_sections.push((path, location.clone()));
                let mut hgid_map: HashMap<Key, NodeLocation> = HashMap::new();
                for (key, hgid_location) in nodes.iter() {
                    let key = Key::new(path.clone(), key.hgid.clone());
                    hgid_map.insert(key, hgid_location.clone());
                }
                file_nodes.insert(path, hgid_map);
            }

            let index = make_index(&file_sections, &file_nodes);

            // Lookup each file section
            for (path, location) in file_sections {
                let my_key = Key::new(path.to_owned(), HgId::null_id().clone());
                let entry = index.get_file_entry(&my_key).unwrap().unwrap();
                assert_eq!(location.offset, entry.file_section_offset);
                assert_eq!(location.size, entry.file_section_size);
            }

            // Lookup each hgid
            for (path, hgid_map) in file_nodes {
                for (key, location) in hgid_map {
                    assert_eq!(path, key.path.as_ref());
                    let entry = index.get_hgid_entry(&key).unwrap().unwrap();
                    assert_eq!(key.hgid, entry.hgid);
                    assert_eq!(location.offset, entry.offset);
                }
            }

            true
        }
    }

    // TODO: test write() when duplicate files and duplicate nodes passed
}

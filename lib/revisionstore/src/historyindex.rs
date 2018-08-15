use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use crypto::digest::Digest;
use crypto::sha1::Sha1;
use memmap::{Mmap, MmapOptions};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use error::{KeyError, Result};
use fanouttable::FanoutTable;
use historypack::HistoryPackVersion;
use key::Key;
use node::Node;

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
    pub fn read<T: Read>(reader: &mut T) -> Result<HistoryIndexOptions> {
        let version = reader.read_u8()?;
        let version = match version {
            0 => HistoryPackVersion::Zero,
            1 => HistoryPackVersion::One,
            _ => {
                return Err(HistoryIndexError(format!("unsupported version '{:?}'", version)).into())
            }
        };

        let raw_config = reader.read_u8()?;
        let large = match raw_config {
            0b10000000 => true,
            0 => false,
            _ => {
                return Err(
                    HistoryIndexError(format!("invalid history index '{:?}'", raw_config)).into(),
                )
            }
        };
        Ok(HistoryIndexOptions { version, large })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<()> {
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
struct FileIndexEntry {
    pub node: Node,
    pub file_section_offset: u64,
    pub file_section_size: u64,
    pub node_index_offset: u32,
    pub node_index_size: u32,
}
const FILE_ENTRY_LEN: usize = 44;

impl FileIndexEntry {
    pub fn read(buf: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(buf);
        cur.set_position(20);
        let node_slice: &[u8] = &buf.get(0..20)
            .ok_or_else(|| HistoryIndexError(format!("buffer too short ({:?} < 20)", buf.len())))?;
        Ok(FileIndexEntry {
            node: Node::from_slice(node_slice)?,
            file_section_offset: cur.read_u64::<BigEndian>()?,
            file_section_size: cur.read_u64::<BigEndian>()?,
            node_index_offset: cur.read_u32::<BigEndian>()?,
            node_index_size: cur.read_u32::<BigEndian>()?,
        })
    }

    fn write<T: Write>(&self, writer: &mut T) -> Result<()> {
        writer.write_all(self.node.as_ref())?;
        writer.write_u64::<BigEndian>(self.file_section_offset)?;
        writer.write_u64::<BigEndian>(self.file_section_size)?;
        writer.write_u32::<BigEndian>(self.node_index_offset)?;
        writer.write_u32::<BigEndian>(self.node_index_size)?;
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
struct NodeIndexEntry {
    pub node: Node,
    pub offset: u64,
}
const NODE_ENTRY_LEN: usize = 28;

impl NodeIndexEntry {
    pub fn read(buf: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(buf);
        cur.set_position(20);
        let node_slice: &[u8] = &buf.get(0..20)
            .ok_or_else(|| HistoryIndexError(format!("buffer too short ({:?} < 20)", buf.len())))?;
        Ok(NodeIndexEntry {
            node: Node::from_slice(node_slice)?,
            offset: cur.read_u64::<BigEndian>()?,
        })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<()> {
        writer.write_all(self.node.as_ref())?;
        writer.write_u64::<BigEndian>(self.offset)?;
        Ok(())
    }
}

pub struct HistoryIndex {
    mmap: Mmap,
    version: HistoryPackVersion,
    fanout_size: usize,
    index_start: usize,
    index_end: usize,
}

impl HistoryIndex {
    pub fn new(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        if len < 1 {
            return Err(HistoryIndexError(format!(
                "empty histidx '{:?}' is invalid",
                path.to_str().unwrap_or("<unknown>")
            )).into());
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
        file_sections: &[(Box<[u8]>, FileSectionLocation)],
        nodes: &HashMap<Box<[u8]>, HashMap<Key, NodeLocation>>,
    ) -> Result<()> {
        // Write header
        let options = HistoryIndexOptions {
            version: HistoryPackVersion::One,
            large: file_sections.len() > SMALL_FANOUT_CUTOFF,
        };
        options.write(writer)?;

        let mut file_sections: Vec<(&Box<[u8]>, Node, FileSectionLocation)> = file_sections
            .iter()
            .map(|e| Ok((&e.0, sha1(&e.0), e.1.clone())))
            .collect::<Result<Vec<(&Box<[u8]>, Node, FileSectionLocation)>>>()?;
        // They must be written in sorted order so they can be bisected.
        file_sections.sort_by_key(|x| x.1);

        // Write the fanout table
        // `locations` is unused by history pack, but we must provide it
        let mut locations: Vec<u32> = Vec::with_capacity(file_sections.len());
        unsafe { locations.set_len(file_sections.len()) };
        FanoutTable::write(
            writer,
            if options.large { 2 } else { 1 },
            &mut file_sections.iter().map(|x| &x.1),
            FILE_ENTRY_LEN,
            &mut locations,
        )?;

        // Write out the number of files in the file portion.
        writer.write_u64::<BigEndian>(file_sections.len() as u64)?;

        <HistoryIndex>::write_file_index(writer, &options, &file_sections, nodes)?;

        // For each file, write a node index
        for &(file_name, ..) in file_sections.iter() {
            <HistoryIndex>::write_node_section(writer, nodes, file_name.as_ref())?;
        }

        Ok(())
    }

    fn write_file_index<T: Write>(
        writer: &mut T,
        options: &HistoryIndexOptions,
        file_sections: &Vec<(&Box<[u8]>, Node, FileSectionLocation)>,
        nodes: &HashMap<Box<[u8]>, HashMap<Key, NodeLocation>>,
    ) -> Result<()> {
        // For each file, keep track of where its node index will start.
        // The first ones starts after the header, fanout, file count, file section, and node count.
        let mut node_offset: usize = 2 + FanoutTable::get_size(options.large) + 8
            + (file_sections.len() * FILE_ENTRY_LEN) + 8;
        let mut node_count = 0;

        // Write out the file section entries
        let mut seen_files = HashSet::<&Box<[u8]>>::with_capacity(file_sections.len());
        for &(file_name, file_hash, ref section_location) in file_sections.iter() {
            if seen_files.contains(file_name) {
                return Err(HistoryIndexError(format!(
                    "file '{:?}' was specified twice",
                    file_name
                )).into());
            }
            seen_files.insert(&file_name);

            let file_nodes: &HashMap<Key, NodeLocation> = nodes.get(file_name).ok_or_else(|| {
                HistoryIndexError(format!("unable to find nodes for {:?}", file_name))
            })?;
            let node_section_size = file_nodes.len() * NODE_ENTRY_LEN;
            FileIndexEntry {
                node: file_hash.clone(),
                file_section_offset: section_location.offset,
                file_section_size: section_location.size,
                node_index_offset: node_offset as u32,
                node_index_size: node_section_size as u32,
            }.write(writer)?;

            // Keep track of the current node index offset
            node_offset += 2 + file_name.len() + node_section_size;
            node_count += file_nodes.len();
        }

        // Write the total number of nodes
        writer.write_u64::<BigEndian>(node_count as u64)?;

        Ok(())
    }

    fn write_node_section<T: Write>(
        writer: &mut T,
        nodes: &HashMap<Box<[u8]>, HashMap<Key, NodeLocation>>,
        file_name: &[u8],
    ) -> Result<()> {
        // Write the filename
        writer.write_u16::<BigEndian>(file_name.len() as u16)?;
        writer.write_all(file_name)?;

        // Write each node, in sorted order so the can be bisected
        let file_nodes = nodes.get(file_name).ok_or_else(|| {
            HistoryIndexError(format!("unabled to find nodes for {:?}", file_name))
        })?;
        let mut file_nodes: Vec<(&Key, &NodeLocation)> =
            file_nodes.iter().collect::<Vec<(&Key, &NodeLocation)>>();
        file_nodes.sort_by_key(|x| x.0.node());

        for &(key, location) in file_nodes.iter() {
            NodeIndexEntry {
                node: key.node().clone(),
                offset: location.offset,
            }.write(writer)?;
        }

        Ok(())
    }

    pub fn get_file_entry(&self, key: &Key) -> Result<FileIndexEntry> {
        let filename_node = sha1(key.name());
        let (start, end) = FanoutTable::get_bounds(self.get_fanout_slice(), &filename_node)?;
        let start = start + self.index_start;
        let end = end.map(|pos| pos + self.index_start)
            .unwrap_or(self.index_end);

        let entry_offset = self.binary_search_files(&filename_node, &self.mmap[start..end])?;
        self.read_file_entry((start + entry_offset) - self.index_start)
    }

    pub fn get_node_entry(&self, key: &Key) -> Result<NodeIndexEntry> {
        let file_entry = self.get_file_entry(&key)?;

        let start = file_entry.node_index_offset as usize + 2 + key.name().len();
        let end = start + file_entry.node_index_size as usize;
        let entry_offset = self.binary_search_nodes(key.node(), &self.mmap[start..end])?;

        self.read_node_entry((start + entry_offset) - self.index_start)
    }

    fn read_file_entry(&self, offset: usize) -> Result<FileIndexEntry> {
        FileIndexEntry::read(self.read_data(offset, FILE_ENTRY_LEN)?)
    }

    fn read_node_entry(&self, offset: usize) -> Result<NodeIndexEntry> {
        NodeIndexEntry::read(self.read_data(offset, NODE_ENTRY_LEN)?)
    }

    fn read_data(&self, offset: usize, size: usize) -> Result<&[u8]> {
        let offset = offset + self.index_start;
        Ok(&self.mmap.get(offset..offset + size).ok_or_else(|| {
            HistoryIndexError(format!(
                "attempted to read offset outside the file (offset {:?} from file len {:?})",
                offset,
                self.mmap.len()
            ))
        })?)
    }

    // These two binary_search_* functions are very similar, but I couldn't find a way to unify
    // them without using macros.
    fn binary_search_files(&self, key: &Node, slice: &[u8]) -> Result<usize> {
        let size = slice.len() / FILE_ENTRY_LEN;
        // Cast the slice into an array of entry buffers so we can bisect across them
        let slice: &[[u8; FILE_ENTRY_LEN]] = unsafe {
            ::std::slice::from_raw_parts(slice.as_ptr() as *const [u8; FILE_ENTRY_LEN], size)
        };
        match slice.binary_search_by(|entry| entry[0..20].cmp(key.as_ref())) {
            Ok(offset) => Ok(offset * FILE_ENTRY_LEN),
            Err(_offset) => Err(KeyError::new(
                HistoryIndexError(format!("no node {:?} in slice", key)).into(),
            ).into()),
        }
    }

    fn binary_search_nodes(&self, key: &Node, slice: &[u8]) -> Result<usize> {
        let size = slice.len() / NODE_ENTRY_LEN;
        // Cast the slice into an array of entry buffers so we can bisect across them
        let slice: &[[u8; NODE_ENTRY_LEN]] = unsafe {
            ::std::slice::from_raw_parts(slice.as_ptr() as *const [u8; NODE_ENTRY_LEN], size)
        };
        match slice.binary_search_by(|entry| entry[0..20].cmp(key.as_ref())) {
            Ok(offset) => Ok(offset * NODE_ENTRY_LEN),
            Err(_offset) => Err(KeyError::new(
                HistoryIndexError(format!("no node {:?} in slice", key)).into(),
            ).into()),
        }
    }

    fn get_fanout_slice(&self) -> &[u8] {
        self.mmap[2..2 + self.fanout_size].as_ref()
    }
}

fn sha1(value: &[u8]) -> Node {
    let mut hasher = Sha1::new();
    hasher.input(value);
    let mut buf: [u8; 20] = Default::default();
    hasher.result(&mut buf);
    (&buf).into()
}

#[cfg(test)]
use quickcheck;

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
    use tempfile::NamedTempFile;

    fn make_index(
        file_sections: &[(Box<[u8]>, FileSectionLocation)],
        nodes: &HashMap<Box<[u8]>, HashMap<Key, NodeLocation>>,
    ) -> HistoryIndex {
        let mut file = NamedTempFile::new().unwrap();
        HistoryIndex::write(&mut file, file_sections, nodes).unwrap();
        let path = file.into_temp_path();

        HistoryIndex::new(&path).unwrap()
    }

    quickcheck! {
        fn test_file_index_entry_roundtrip(
            node: Node,
            file_section_offset: u64,
            file_section_size: u64,
            node_index_offset: u32,
            node_index_size: u32
        ) -> bool {
            let entry = FileIndexEntry {
                node,
                file_section_offset,
                file_section_size,
                node_index_offset,
                node_index_size,
            };

            let mut buf: Vec<u8> = vec![];
            entry.write(&mut buf).unwrap();
            entry == FileIndexEntry::read(buf.as_ref()).unwrap()
        }

        fn test_node_index_entry_roundtrip(node: Node, offset: u64) -> bool {
            let entry = NodeIndexEntry {
                node, offset
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

        fn test_roundtrip_index(data: Vec<(Vec<u8>, (FileSectionLocation, HashMap<Key, NodeLocation>))>) -> bool {
            let mut file_sections: Vec<(Box<[u8]>, FileSectionLocation)> = vec![];
            let mut file_nodes: HashMap<Box<[u8]>, HashMap<Key, NodeLocation>> = HashMap::new();

            let mut seen_files: HashSet<Box<[u8]>> = HashSet::new();
            for &(ref name_vec, (ref location, ref nodes)) in data.iter() {
                let name_slice = name_vec.clone().into_boxed_slice();

                // Don't allow a filename to be used twice
                if seen_files.contains(&name_slice) {
                    continue;
                }
                seen_files.insert(name_slice.clone());

                file_sections.push((name_slice.clone(), location.clone()));
                let mut node_map: HashMap<Key, NodeLocation> = HashMap::new();
                for (key, node_location) in nodes.iter() {
                    let key = Key::new(name_slice.clone(), key.node().clone());
                    node_map.insert(key, node_location.clone());
                }
                file_nodes.insert(name_slice.clone(), node_map);
            }

            let index = make_index(&file_sections, &file_nodes);

            // Lookup each file section
            for &(ref name, ref location) in file_sections.iter() {
                let entry = index.get_file_entry(&Key::new(name.clone(), Node::null_id().clone())).unwrap();
                assert_eq!(location.offset, entry.file_section_offset);
                assert_eq!(location.size, entry.file_section_size);
            }

            // Lookup each node
            for (ref name, ref node_map) in file_nodes.iter() {
                for (ref key, ref location) in node_map.iter() {
                    assert_eq!(name.as_ref(), key.name());
                    let entry = index.get_node_entry(key).unwrap();
                    assert_eq!(key.node(), &entry.node);
                    assert_eq!(location.offset, entry.offset);
                }
            }

            true
        }
    }

    // TODO: test write() when duplicate files and duplicate nodes passed
}

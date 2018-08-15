use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read, Write};

use error::Result;
use historypack::HistoryPackVersion;
use node::Node;

#[derive(Debug, Fail)]
#[fail(display = "HistoryIndex Error: {:?}", _0)]
struct HistoryIndexError(String);

#[derive(Debug, PartialEq)]
struct HistoryIndexOptions {
    version: HistoryPackVersion,
    // Indicates whether to use the large fanout (2 bytes) or the small (1 byte)
    large: bool,
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

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read, Write};

use error::Result;
use node::Node;

#[derive(Debug, Fail)]
#[fail(display = "Historypack Error: {:?}", _0)]
struct HistoryPackError(String);

#[derive(Clone, Debug, PartialEq)]
pub enum HistoryPackVersion {
    Zero,
    One,
}

impl HistoryPackVersion {
    fn new(value: u8) -> Result<Self> {
        match value {
            0 => Ok(HistoryPackVersion::Zero),
            1 => Ok(HistoryPackVersion::One),
            _ => Err(HistoryPackError(format!(
                "invalid history pack version number '{:?}'",
                value
            )).into()),
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
struct FileSectionHeader<'a> {
    pub file_name: &'a [u8],
    pub count: u32,
}

#[derive(Debug, PartialEq)]
pub struct HistoryEntry<'a> {
    pub node: Node,
    pub p1: Node,
    pub p2: Node,
    pub link_node: Node,
    pub copy_from: Option<&'a [u8]>,
}

fn read_slice<'a, 'b>(cur: &'a mut Cursor<&[u8]>, buf: &'b [u8], size: usize) -> Result<&'b [u8]> {
    let start = cur.position() as usize;
    let end = start + size;
    let file_name = &buf.get(start..end).ok_or_else(|| {
        HistoryPackError(format!(
            "buffer (length {:?}) not long enough to read {:?} bytes",
            buf.len(),
            size
        ))
    })?;
    cur.set_position(end as u64);
    Ok(file_name)
}

impl<'a> FileSectionHeader<'a> {
    pub(crate) fn read(buf: &[u8]) -> Result<FileSectionHeader> {
        let mut cur = Cursor::new(buf);
        let file_name_len = cur.read_u16::<BigEndian>()? as usize;
        let file_name = read_slice(&mut cur, &buf, file_name_len)?;

        let count = cur.read_u32::<BigEndian>()?;
        Ok(FileSectionHeader { file_name, count })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<()> {
        writer.write_u16::<BigEndian>(self.file_name.len() as u16)?;
        writer.write_all(self.file_name)?;
        writer.write_u32::<BigEndian>(self.count)?;
        Ok(())
    }
}

impl<'a> HistoryEntry<'a> {
    pub(crate) fn read(buf: &[u8]) -> Result<HistoryEntry> {
        let mut cur = Cursor::new(buf);
        let mut node_buf: [u8; 20] = Default::default();

        // Node
        cur.read_exact(&mut node_buf)?;
        let node = Node::from(&node_buf);

        // Parents
        cur.read_exact(&mut node_buf)?;
        let p1 = Node::from(&node_buf);
        cur.read_exact(&mut node_buf)?;
        let p2 = Node::from(&node_buf);

        // LinkNode
        cur.read_exact(&mut node_buf)?;
        let link_node = Node::from(&node_buf);

        // Copyfrom
        let copy_from_len = cur.read_u16::<BigEndian>()? as usize;
        let copy_from = if copy_from_len > 0 {
            Some(read_slice(&mut cur, &buf, copy_from_len)?)
        } else {
            None
        };

        Ok(HistoryEntry {
            node,
            p1,
            p2,
            link_node,
            copy_from,
        })
    }

    pub fn write<T: Write>(
        writer: &mut T,
        node: &Node,
        p1: &Node,
        p2: &Node,
        linknode: &Node,
        copy_from: &Option<&[u8]>,
    ) -> Result<()> {
        writer.write_all(node.as_ref())?;
        writer.write_all(p1.as_ref())?;
        writer.write_all(p2.as_ref())?;
        writer.write_all(linknode.as_ref())?;
        match copy_from {
            Some(file_name) => {
                writer.write_u16::<BigEndian>(file_name.len() as u16)?;
                writer.write_all(file_name)?;
            }
            None => writer.write_u16::<BigEndian>(0)?,
        };

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    quickcheck! {
        fn test_file_section_header_serialization(name: Vec<u8>, count: u32) -> bool {
            let header = FileSectionHeader {
                file_name: name.as_ref(),
                count: count,
            };
            let mut buf = vec![];
            header.write(&mut buf).unwrap();
            header == FileSectionHeader::read(&buf).unwrap()
        }

        fn test_history_entry_serialization(
            node: Node,
            p1: Node,
            p2: Node,
            link_node: Node,
            copy_from: Option<Vec<u8>>
        ) -> bool {
            let mut buf = vec![];
            HistoryEntry::write(
                &mut buf,
                &node,
                &p1,
                &p2,
                &link_node,
                &copy_from.as_ref().map(|x| x.as_ref()),
            ).unwrap();
            let entry = HistoryEntry::read(&buf).unwrap();
            assert_eq!(node, entry.node);
            assert_eq!(p1, entry.p1);
            assert_eq!(p2, entry.p2);
            assert_eq!(link_node, entry.link_node);
            true
        }
    }
}

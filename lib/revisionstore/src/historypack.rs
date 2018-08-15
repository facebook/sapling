use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read, Write};

use error::Result;

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
    }
}

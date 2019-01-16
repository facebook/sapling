// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use error::Result;
use key::Key;
use std::io::{Cursor, Write};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub struct Delta {
    pub data: Rc<[u8]>,
    pub base: Option<Key>,
    pub key: Key,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metadata {
    pub size: Option<u64>,
    pub flags: Option<u64>,
}

pub trait DataStore {
    fn get(&self, key: &Key) -> Result<Vec<u8>>;
    fn get_delta(&self, key: &Key) -> Result<Delta>;
    fn get_delta_chain(&self, key: &Key) -> Result<Vec<Delta>>;
    fn get_meta(&self, key: &Key) -> Result<Metadata>;
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>>;

    fn contains(&self, key: &Key) -> Result<bool> {
        Ok(self.get_missing(&[key.clone()])?.is_empty())
    }
}

impl Metadata {
    pub fn write<T: Write>(&self, writer: &mut T) -> Result<()> {
        let mut buf = vec![];
        if let Some(flags) = self.flags {
            Metadata::write_meta(b'f', flags, &mut buf)?;
        }
        if let Some(size) = self.size {
            Metadata::write_meta(b's', size, &mut buf)?;
        }

        writer.write_u32::<BigEndian>(buf.len() as u32)?;
        writer.write_all(buf.as_ref())?;
        Ok(())
    }

    fn write_meta<T: Write>(flag: u8, value: u64, writer: &mut T) -> Result<()> {
        writer.write_u8(flag as u8)?;
        writer.write_u16::<BigEndian>(u64_to_bin_len(value))?;
        u64_to_bin(value, writer)?;
        Ok(())
    }

    pub fn read(cur: &mut Cursor<&[u8]>) -> Result<Metadata> {
        let metadata_len = cur.read_u32::<BigEndian>()? as u64;
        let mut size: Option<u64> = None;
        let mut flags: Option<u64> = None;
        let start_offset = cur.position();
        while cur.position() < start_offset + metadata_len {
            let key = cur.read_u8()?;
            let value_len = cur.read_u16::<BigEndian>()? as usize;
            match key {
                b'f' => {
                    let buf = cur.get_ref();
                    flags = Some(bin_to_u64(
                        &buf[cur.position() as usize..cur.position() as usize + value_len],
                    ));
                }
                b's' => {
                    let buf = cur.get_ref();
                    size = Some(bin_to_u64(
                        &buf[cur.position() as usize..cur.position() as usize + value_len],
                    ));
                }
                _ => return Err(format_err!("invalid metadata format '{:?}'", key)),
            }

            let cur_pos = cur.position();
            cur.set_position(cur_pos + value_len as u64);
        }

        Ok(Metadata { flags, size })
    }
}

/// Precompute the size of a u64 when it is serialized
fn u64_to_bin_len(value: u64) -> u16 {
    let mut value = value;
    let mut count = 0;
    while value > 0 {
        count += 1;
        value >>= 8;
    }
    count
}

/// Converts an integer into a buffer using a special format used in the datapack format.
fn u64_to_bin<T: Write>(value: u64, writer: &mut T) -> Result<()> {
    let mut value = value;
    let mut buf = [0; 8];
    let len = u64_to_bin_len(value) as usize;
    let mut pos = len;
    while value > 0 {
        pos -= 1;
        buf[pos] = value as u8;
        value >>= 8;
    }
    assert!(value == 0 && pos == 0);

    writer.write_all(&buf[0..len])?;
    Ok(())
}

/// Converts a buffer to an integer using a special format used in the datapack format.
fn bin_to_u64(buf: &[u8]) -> u64 {
    let mut n: u64 = 0;
    for byte in buf.iter() {
        n <<= 8;
        n |= *byte as u64;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip_meta_serialize(meta: &Metadata) {
        let mut buf = vec![];
        meta.write(&mut buf).expect("write");
        let read_meta = Metadata::read(&mut Cursor::new(&buf)).expect("meta");
        assert!(*meta == read_meta);
    }

    #[test]
    fn test_metadata_serialize() {
        roundtrip_meta_serialize(&Metadata {
            size: None,
            flags: None,
        });
        roundtrip_meta_serialize(&Metadata {
            size: Some(5),
            flags: None,
        });
        roundtrip_meta_serialize(&Metadata {
            size: Some(0),
            flags: Some(12),
        });
        roundtrip_meta_serialize(&Metadata {
            size: Some(1000),
            flags: Some(12),
        });
        roundtrip_meta_serialize(&Metadata {
            size: Some(234214134),
            flags: Some(9879489),
        });
    }

    quickcheck! {
        fn test_roundtrip_bin_to_u64(value: u64) -> bool {
            let mut buf: Vec<u8> = vec![];
            u64_to_bin(value, &mut buf).unwrap();
            if buf.len() != u64_to_bin_len(value) as usize {
                return false;
            }
            let new_value = bin_to_u64(&buf);
            value == new_value
        }

        fn test_roundtrip_metadata(size: Option<u64>, flags: Option<u64>) -> bool {
            let meta = Metadata { size, flags };
            let mut buf: Vec<u8> = vec![];
            meta.write(&mut buf).expect("write");
            let read_meta = Metadata::read(&mut Cursor::new(&buf)).expect("read");
            meta == read_meta
        }
    }
}

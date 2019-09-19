// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{
    io::{Cursor, Write},
    ops::Deref,
    path::PathBuf,
};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use bytes::Bytes;
use failure::{format_err, Fallible};
use serde_derive::{Deserialize, Serialize};

use types::Key;

use crate::localstore::LocalStore;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    pub data: Bytes,
    pub base: Option<Key>,
    pub key: Key,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    pub size: Option<u64>,
    pub flags: Option<u64>,
}

pub trait DataStore: LocalStore {
    fn get(&self, key: &Key) -> Fallible<Vec<u8>>;
    fn get_delta(&self, key: &Key) -> Fallible<Delta>;
    fn get_delta_chain(&self, key: &Key) -> Fallible<Vec<Delta>>;
    fn get_meta(&self, key: &Key) -> Fallible<Metadata>;
}

/// The `RemoteDataStore` trait indicates that data can fetched over the network. Care must be
/// taken to avoid serially fetching data and instead data should be fetched in bulk via the
/// `prefetch` API.
pub trait RemoteDataStore {
    /// Attempt to bring the data corresponding to the passed in keys to a local store.
    ///
    /// When implemented on a pure remote store, like the `EdenApi`, the method will always fetch
    /// everything that was asked. On a higher level store, such as the `ContentStore`, this will
    /// avoid fetching data that is already present locally.
    fn prefetch(&self, keys: Vec<Key>) -> Fallible<()>;
}

pub trait MutableDeltaStore: DataStore {
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Fallible<()>;
    fn flush(&self) -> Fallible<Option<PathBuf>>;
}

/// Implement `DataStore` for all types that can be `Deref` into a `DataStore`. This includes all
/// the smart pointers like `Box`, `Rc`, `Arc`.
impl<T: DataStore + ?Sized, U: Deref<Target = T>> DataStore for U {
    fn get(&self, key: &Key) -> Fallible<Vec<u8>> {
        T::get(self, key)
    }
    fn get_delta(&self, key: &Key) -> Fallible<Delta> {
        T::get_delta(self, key)
    }
    fn get_delta_chain(&self, key: &Key) -> Fallible<Vec<Delta>> {
        T::get_delta_chain(self, key)
    }
    fn get_meta(&self, key: &Key) -> Fallible<Metadata> {
        T::get_meta(self, key)
    }
}

/// Implement `RemoteDataStore` for all types that can be `Deref` into a `RemoteDataStore`. This
/// includes all the smart pointers like `Box`, `Rc`, `Arc`.
impl<T: RemoteDataStore + ?Sized, U: Deref<Target = T>> RemoteDataStore for U {
    fn prefetch(&self, keys: Vec<Key>) -> Fallible<()> {
        T::prefetch(self, keys)
    }
}

impl<T: MutableDeltaStore + ?Sized, U: Deref<Target = T>> MutableDeltaStore for U {
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Fallible<()> {
        T::add(self, delta, metadata)
    }

    fn flush(&self) -> Fallible<Option<PathBuf>> {
        T::flush(self)
    }
}

impl Metadata {
    pub fn write<T: Write>(&self, writer: &mut T) -> Fallible<()> {
        let mut buf = vec![];
        if let Some(flags) = self.flags {
            if flags != 0 {
                Metadata::write_meta(b'f', flags, &mut buf)?;
            }
        }
        if let Some(size) = self.size {
            Metadata::write_meta(b's', size, &mut buf)?;
        }

        writer.write_u32::<BigEndian>(buf.len() as u32)?;
        writer.write_all(buf.as_ref())?;
        Ok(())
    }

    fn write_meta<T: Write>(flag: u8, value: u64, writer: &mut T) -> Fallible<()> {
        writer.write_u8(flag as u8)?;
        writer.write_u16::<BigEndian>(u64_to_bin_len(value))?;
        u64_to_bin(value, writer)?;
        Ok(())
    }

    pub fn read(cur: &mut Cursor<&[u8]>) -> Fallible<Metadata> {
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
fn u64_to_bin<T: Write>(value: u64, writer: &mut T) -> Fallible<()> {
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
    use quickcheck::quickcheck;

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

            meta.size == read_meta.size && (meta.flags == read_meta.flags || meta.flags.map_or(false, |v| v == 0))
        }
    }
}

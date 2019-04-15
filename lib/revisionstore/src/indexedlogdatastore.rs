// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{
    io::{Cursor, Seek, SeekFrom, Write},
    path::Path,
};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use failure::{format_err, Fallible};

use indexedlog::{
    log::IndexOutput,
    rotate::{LogRotate, OpenOptions},
};
use lz4_pyframe::{compress, decompress};
use types::{node::ReadNodeExt, Key, Node};

use crate::{
    datastore::{DataStore, Delta, Metadata},
    sliceext::SliceExt,
    store::Store,
};

pub struct IndexedLogDataStore {
    log: LogRotate,
}

struct Entry {
    pub delta: Delta,
    pub metadata: Metadata,
}

impl Entry {
    pub fn new(delta: Delta, metadata: Metadata) -> Self {
        Entry { delta, metadata }
    }

    pub fn from_log(key: &Key, log: &LogRotate) -> Fallible<Self> {
        let mut log_entry = log.lookup(0, key.node.as_ref())?;
        let buf = log_entry.nth(0).ok_or_else(|| format_err!("Not found"))??;

        let mut cur = Cursor::new(buf);
        cur.seek(SeekFrom::Current(Node::len() as i64))?;

        let name_len = cur.read_u16::<BigEndian>()? as i64;
        cur.seek(SeekFrom::Current(name_len))?;

        let base = cur.read_node()?;
        let base = if base.is_null() {
            None
        } else {
            Some(Key::new(key.name().to_vec(), base))
        };

        let metadata = Metadata::read(&mut cur)?;

        let compressed_len = cur.read_u64::<BigEndian>()?;
        let compressed =
            buf.get_err(cur.position() as usize..(cur.position() + compressed_len) as usize)?;

        let delta = Delta {
            // XXX: Only decompress on-demand.
            data: decompress(&compressed)?.into(),
            base,
            key: key.clone(),
        };

        Ok(Entry { delta, metadata })
    }

    pub fn write_to_log(self, log: &mut LogRotate) -> Fallible<()> {
        let mut buf = Vec::new();
        buf.write_all(self.delta.key.node.as_ref())?;
        buf.write_u16::<BigEndian>(self.delta.key.name().len() as u16)?;
        buf.write_all(self.delta.key.name())?;
        buf.write_all(
            self.delta
                .base
                .as_ref()
                .map_or_else(|| Node::null_id(), |k| &k.node)
                .as_ref(),
        )?;
        self.metadata.write(&mut buf)?;

        let compressed = compress(&self.delta.data)?;
        buf.write_u64::<BigEndian>(compressed.len() as u64)?;
        buf.write_all(&compressed)?;

        Ok(log.append(buf)?)
    }
}

impl IndexedLogDataStore {
    pub fn new(path: impl AsRef<Path>) -> Fallible<Self> {
        let log = OpenOptions::new()
            .max_log_count(10)
            .max_bytes_per_log(1 * 1024 * 1024 * 1024)
            .create(true)
            .index("node", |_| {
                vec![IndexOutput::Reference(0..Node::len() as u64)]
            })
            .open(path)?;
        Ok(IndexedLogDataStore { log })
    }

    pub fn add(&mut self, delta: &Delta, metadata: &Metadata) -> Fallible<()> {
        let entry = Entry::new(delta.clone(), metadata.clone());
        entry.write_to_log(&mut self.log)
    }

    pub fn flush(&mut self) -> Fallible<()> {
        self.log.flush()?;
        Ok(())
    }

    pub fn close(mut self) -> Fallible<()> {
        self.flush()
    }
}

impl Store for IndexedLogDataStore {
    fn from_path(path: &Path) -> Fallible<Self> {
        IndexedLogDataStore::new(path)
    }

    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        Ok(keys
            .iter()
            .filter(|k| Entry::from_log(k, &self.log).is_err())
            .map(|k| k.clone())
            .collect())
    }
}

impl DataStore for IndexedLogDataStore {
    fn get(&self, _key: &Key) -> Fallible<Vec<u8>> {
        unreachable!()
    }

    fn get_delta(&self, key: &Key) -> Fallible<Delta> {
        Ok(Entry::from_log(&key, &self.log)?.delta)
    }

    fn get_delta_chain(&self, key: &Key) -> Fallible<Vec<Delta>> {
        let mut chain = Vec::new();
        let mut next_key = Some(key.clone());

        while let Some(key) = next_key {
            let entry = Entry::from_log(&key, &self.log)?;
            next_key = entry.delta.base.clone();
            chain.push(entry.delta);
        }

        Ok(chain)
    }

    fn get_meta(&self, key: &Key) -> Fallible<Metadata> {
        Ok(Entry::from_log(&key, &self.log)?.metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use bytes::Bytes;
    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;
    use tempfile::TempDir;

    #[test]
    fn test_empty() {
        let tempdir = TempDir::new().unwrap();
        let log = IndexedLogDataStore::new(&tempdir).unwrap();
        log.close().unwrap();
    }

    #[test]
    fn test_add() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new().unwrap();
        let mut log = IndexedLogDataStore::new(&tempdir).unwrap();

        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: Key::new(vec![0], Node::random(&mut rng)),
        };
        let metadata = Default::default();

        log.add(&delta, &metadata).unwrap();
        log.close().unwrap();
    }

    #[test]
    fn test_add_get() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new().unwrap();
        let mut log = IndexedLogDataStore::new(&tempdir).unwrap();

        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: Key::new(vec![0], Node::random(&mut rng)),
        };
        let metadata = Default::default();

        log.add(&delta, &metadata).unwrap();
        log.close().unwrap();

        let log = IndexedLogDataStore::new(&tempdir).unwrap();
        let read_delta = log.get_delta(&delta.key).unwrap();
        assert_eq!(delta, read_delta);
    }
}

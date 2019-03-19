// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    u16,
};

use byteorder::{BigEndian, WriteBytesExt};
use crypto::digest::Digest;
use crypto::sha1::Sha1;
use failure::{format_err, Error, Fail, Fallible};
use tempfile::NamedTempFile;

use lz4_pyframe::compress;
use types::{Key, Node};

use crate::dataindex::{DataIndex, DeltaLocation};
use crate::datapack::{DataEntry, DataPackVersion};
use crate::datastore::{DataStore, Delta, Metadata};
use crate::mutablepack::MutablePack;
use crate::packwriter::PackWriter;
use crate::store::Store;

pub struct MutableDataPack {
    dir: PathBuf,
    data_file: PackWriter<NamedTempFile>,
    mem_index: HashMap<Node, DeltaLocation>,
    hasher: Sha1,
}

#[derive(Debug, Fail)]
#[fail(display = "Mutable Data Pack Error: {:?}", _0)]
struct MutableDataPackError(String);

impl MutableDataPack {
    /// Creates a new MutableDataPack for producing datapack files.
    ///
    /// The data is written to a temporary file, and renamed to the final location
    /// when close() is called, at which point the MutableDataPack is consumed. If
    /// close() is not called, the temporary file is cleaned up when the object is
    /// release.
    pub fn new(dir: impl AsRef<Path>, version: DataPackVersion) -> Fallible<Self> {
        let dir = dir.as_ref();
        if !dir.is_dir() {
            return Err(format_err!(
                "cannot create mutable datapack in non-directory '{:?}'",
                dir
            ));
        }

        if version == DataPackVersion::Zero {
            return Err(format_err!("cannot create a v0 datapack"));
        }

        let mut data_file = PackWriter::new(NamedTempFile::new_in(&dir)?);
        let mut hasher = Sha1::new();
        let version_u8: u8 = version.clone().into();
        data_file.write_u8(version_u8)?;
        hasher.input(&[version_u8]);

        Ok(MutableDataPack {
            dir: dir.to_path_buf(),
            data_file,
            mem_index: HashMap::new(),
            hasher,
        })
    }

    /// Adds the given entry to the mutable datapack.
    pub fn add(&mut self, delta: &Delta, metadata: &Metadata) -> Fallible<()> {
        if delta.key.name().len() >= u16::MAX as usize {
            return Err(MutableDataPackError("delta name is longer than 2^16".into()).into());
        }

        let offset = self.data_file.bytes_written();

        let compressed = compress(&delta.data)?;

        // Preallocate with approximately the size we need:
        // (namelen(2) + name + node(20) + node(20) + datalen(8) + data + metadata(~22))
        let mut buf = Vec::with_capacity(delta.key.name().len() + compressed.len() + 72);
        buf.write_u16::<BigEndian>(delta.key.name().len() as u16)?;
        buf.write_all(delta.key.name())?;
        buf.write_all(delta.key.node().as_ref())?;

        buf.write_all(
            delta
                .base
                .as_ref()
                .map_or_else(|| Node::null_id(), |k| k.node())
                .as_ref(),
        )?;
        buf.write_u64::<BigEndian>(compressed.len() as u64)?;
        buf.write_all(&compressed)?;

        metadata.write(&mut buf)?;

        self.data_file.write_all(&buf)?;
        self.hasher.input(&buf);

        let delta_location = DeltaLocation {
            delta_base: delta.base.as_ref().map_or(None, |k| Some(k.node().clone())),
            offset,
            size: buf.len() as u64,
        };
        self.mem_index
            .insert(delta.key.node().clone(), delta_location);
        Ok(())
    }

    fn read_entry(&self, key: &Key) -> Fallible<(Delta, Metadata)> {
        let location: &DeltaLocation = self.mem_index.get(key.node()).ok_or::<Error>(
            MutableDataPackError(format!("Unable to find key {:?} in mutable datapack", key))
                .into(),
        )?;
        // Make sure the buffers are empty so the reads below are consistent with what is being
        // written.
        self.data_file.flush_inner()?;
        let mut file = self.data_file.get_mut();

        let mut data = Vec::with_capacity(location.size as usize);
        unsafe { data.set_len(location.size as usize) };

        file.seek(SeekFrom::Start(location.offset))?;
        file.read_exact(&mut data)?;
        // The add function assumes the file position is always at the end, so reset it.
        file.seek(SeekFrom::End(0))?;

        let entry = DataEntry::new(&data, 0, DataPackVersion::One)?;
        Ok((
            Delta {
                data: entry.delta()?,
                base: entry
                    .delta_base()
                    .map(|delta_base| Key::new(key.name().into(), delta_base.clone())),
                key: Key::new(key.name().into(), entry.node().clone()),
            },
            entry.metadata().clone(),
        ))
    }
}

impl MutablePack for MutableDataPack {
    fn build_files(mut self) -> Fallible<(NamedTempFile, NamedTempFile, PathBuf)> {
        let mut index_file = PackWriter::new(NamedTempFile::new_in(&self.dir)?);
        DataIndex::write(&mut index_file, &self.mem_index)?;

        Ok((
            self.data_file.into_inner()?,
            index_file.into_inner()?,
            self.dir.join(&self.hasher.result_str()),
        ))
    }

    fn extension(&self) -> &'static str {
        "data"
    }
}

impl DataStore for MutableDataPack {
    fn get(&self, _key: &Key) -> Fallible<Vec<u8>> {
        Err(
            MutableDataPackError("DataPack doesn't support raw get(), only getdeltachain".into())
                .into(),
        )
    }

    fn get_delta(&self, key: &Key) -> Fallible<Delta> {
        let (delta, _metadata) = self.read_entry(&key)?;
        Ok(delta)
    }

    fn get_delta_chain(&self, key: &Key) -> Fallible<Vec<Delta>> {
        let mut chain: Vec<Delta> = Default::default();
        let mut next_key = Some(key.clone());
        while let Some(key) = next_key {
            let (delta, _metadata) = self.read_entry(&key)?;
            next_key = delta.base.clone();
            chain.push(delta);
        }

        Ok(chain)
    }

    fn get_meta(&self, key: &Key) -> Fallible<Metadata> {
        let (_, metadata) = self.read_entry(&key)?;
        Ok(metadata)
    }
}

impl Store for MutableDataPack {
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        Ok(keys
            .iter()
            .filter(|k| self.mem_index.get(k.node()).is_none())
            .map(|k| k.clone())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{
        fs::{self, File},
        io::Read,
    };

    use bytes::Bytes;
    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;
    use tempfile::tempdir;

    use types::Key;

    #[test]
    fn test_basic_creation() {
        let tempdir = tempdir().unwrap();
        let mut mutdatapack = MutableDataPack::new(tempdir.path(), DataPackVersion::One).unwrap();
        let delta = Delta {
            data: Bytes::from(&[0, 1, 2][..]),
            base: None,
            key: Key::new(Vec::new(), Default::default()),
        };
        mutdatapack.add(&delta, &Default::default()).expect("add");
        let datapackbase = mutdatapack.close().expect("close");
        let datapackpath = datapackbase.with_extension("datapack");
        let dataindexpath = datapackbase.with_extension("dataidx");

        assert!(datapackpath.exists());
        assert!(dataindexpath.exists());

        // Verify the hash
        let mut temppath = datapackpath.clone();
        // The file's name is the hash of it's content, so drop the extension to get just the name
        temppath.set_extension("");

        let filename_hash = temppath.file_name().unwrap().to_str().unwrap();
        let mut hasher = Sha1::new();
        let mut file = File::open(datapackpath).expect("file");
        let mut buf = vec![];
        file.read_to_end(&mut buf).expect("read to end");
        hasher.input(&buf);
        let hash = hasher.result_str();
        assert!(hash == filename_hash);
    }

    #[test]
    fn test_basic_abort() {
        let tempdir = tempdir().unwrap();
        {
            let mut mutdatapack =
                MutableDataPack::new(tempdir.path(), DataPackVersion::One).unwrap();
            let delta = Delta {
                data: Bytes::from(&[0, 1, 2][..]),
                base: None,
                key: Key::new(Vec::new(), Default::default()),
            };
            mutdatapack.add(&delta, &Default::default()).expect("add");
        }

        assert_eq!(fs::read_dir(tempdir.path()).unwrap().count(), 0);
    }

    #[test]
    fn test_get_delta_chain() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);

        let tempdir = tempdir().unwrap();
        let mut mutdatapack = MutableDataPack::new(tempdir.path(), DataPackVersion::One).unwrap();
        let delta = Delta {
            data: Bytes::from(&[0, 1, 2][..]),
            base: None,
            key: Key::new(Vec::new(), Node::random(&mut rng)),
        };
        mutdatapack.add(&delta, &Default::default()).unwrap();
        let delta2 = Delta {
            data: Bytes::from(&[0, 1, 2][..]),
            base: Some(Key::new(Vec::new(), delta.key.node().clone())),
            key: Key::new(Vec::new(), Node::random(&mut rng)),
        };
        mutdatapack.add(&delta2, &Default::default()).unwrap();

        let chain = mutdatapack.get_delta_chain(&delta.key).unwrap();
        assert_eq!(&vec![delta.clone()], &chain);

        let chain = mutdatapack.get_delta_chain(&delta2.key).unwrap();
        assert_eq!(&vec![delta2.clone(), delta.clone()], &chain);
    }

    #[test]
    fn test_get_meta() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = tempdir().unwrap();

        let mut mutdatapack = MutableDataPack::new(tempdir.path(), DataPackVersion::One).unwrap();
        let delta = Delta {
            data: Bytes::from(&[0, 1, 2][..]),
            base: None,
            key: Key::new(Vec::new(), Node::random(&mut rng)),
        };
        mutdatapack.add(&delta, &Default::default()).unwrap();
        let delta2 = Delta {
            data: Bytes::from(&[0, 1, 2][..]),
            base: None,
            key: Key::new(Vec::new(), Node::random(&mut rng)),
        };
        let meta2 = Metadata {
            flags: Some(2),
            size: Some(1000),
        };
        mutdatapack.add(&delta2, &meta2).unwrap();

        // Requesting a default metadata
        let found_meta = mutdatapack.get_meta(&delta.key).unwrap();
        assert_eq!(found_meta, Metadata::default());

        // Requesting a specified metadata
        let found_meta = mutdatapack.get_meta(&delta2.key).unwrap();
        assert_eq!(found_meta, meta2);

        // Requesting a non-existent metadata
        let not = Key::new(vec![1], Node::random(&mut rng));
        mutdatapack
            .get_meta(&not)
            .expect_err("expected error for non existent node");
    }

    #[test]
    fn test_get_missing() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = tempdir().unwrap();

        let mut mutdatapack = MutableDataPack::new(tempdir.path(), DataPackVersion::One).unwrap();
        let delta = Delta {
            data: Bytes::from(&[0, 1, 2][..]),
            base: None,
            key: Key::new(Vec::new(), Default::default()),
        };
        mutdatapack.add(&delta, &Default::default()).unwrap();

        let not = Key::new(vec![1], Node::random(&mut rng));
        let missing = mutdatapack
            .get_missing(&vec![delta.key.clone(), not.clone()])
            .unwrap();
        assert_eq!(missing, vec![not.clone()]);
    }
}

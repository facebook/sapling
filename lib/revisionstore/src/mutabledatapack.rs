use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::u16;

use byteorder::{BigEndian, WriteBytesExt};
use crypto::digest::Digest;
use crypto::sha1::Sha1;
use dataindex::{DataIndex, DeltaLocation};
use datapack::DataEntry;
use datastore::{DataStore, Delta, Metadata};
use failure::Error;
use key::Key;
use lz4_pyframe::compress;
use node::Node;
use tempfile::NamedTempFile;

use error::Result;

pub struct MutableDataPack {
    version: u8,
    dir: PathBuf,
    data_file: NamedTempFile,
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
    pub fn new(dir: &Path, version: u8) -> Result<Self> {
        if !dir.is_dir() {
            return Err(format_err!(
                "cannot create mutable datapack in non-directory '{:?}'",
                dir
            ));
        }

        let mut data_file = NamedTempFile::new_in(&dir)?;
        let mut hasher = Sha1::new();
        data_file.write_u8(version)?;
        hasher.input(&[version]);

        Ok(MutableDataPack {
            version: version,
            dir: dir.to_path_buf(),
            data_file: data_file,
            mem_index: HashMap::new(),
            hasher: hasher,
        })
    }

    /// Closes the mutable datapack, returning the path of the final immutable datapack on disk.
    /// The mutable datapack is no longer usable after being closed.
    pub fn close(mut self) -> Result<PathBuf> {
        // Compute the index
        let mut index_file = NamedTempFile::new_in(&self.dir)?;
        DataIndex::write(&mut index_file, &self.mem_index)?;

        // Persist the temp files
        let base_filepath = self.dir.join(&self.hasher.result_str());
        let data_filepath = base_filepath.with_extension("datapack");
        let index_filepath = base_filepath.with_extension("dataidx");

        self.data_file.persist(&data_filepath)?;
        index_file.persist(&index_filepath)?;
        Ok(base_filepath)
    }

    /// Adds the given entry to the mutable datapack.
    pub fn add(&mut self, delta: &Delta, metadata: Option<Metadata>) -> Result<()> {
        if delta.key.name().len() >= u16::MAX as usize {
            return Err(MutableDataPackError("delta name is longer than 2^16".into()).into());
        }
        if self.version == 0 && metadata.is_some() {
            return Err(MutableDataPackError("v0 data pack cannot store metadata".into()).into());
        }

        let offset = self.data_file.as_ref().metadata()?.len();

        let compressed = compress(&delta.data)?;

        // Preallocate with approximately the size we need:
        // (namelen(2) + name + node(20) + node(20) + datalen(8) + data + metadata(~22))
        let mut buf = Vec::with_capacity(delta.key.name().len() + compressed.len() + 72);
        buf.write_u16::<BigEndian>(delta.key.name().len() as u16)?;
        buf.write_all(delta.key.name())?;
        buf.write_all(delta.key.node().as_ref())?;
        buf.write_all(delta.base.node().as_ref())?;
        buf.write_u64::<BigEndian>(compressed.len() as u64)?;
        buf.write_all(&compressed)?;

        if self.version == 1 {
            metadata.unwrap_or_default().write(&mut buf)?;
        }

        self.data_file.write_all(&buf)?;
        self.hasher.input(&buf);

        let delta_location = DeltaLocation {
            delta_base: delta.base.node().clone(),
            offset: offset,
            size: buf.len() as u64,
        };
        self.mem_index
            .insert(delta.key.node().clone(), delta_location);
        Ok(())
    }

    fn read_entry(&self, key: &Key) -> Result<(Delta, Metadata)> {
        let location: &DeltaLocation = self.mem_index.get(key.node()).ok_or::<Error>(
            MutableDataPackError(format!("Unable to find key {:?} in mutable datapack", key))
                .into(),
        )?;
        let mut file = &self.data_file;

        let mut data = Vec::with_capacity(location.size as usize);
        unsafe { data.set_len(location.size as usize) };

        file.seek(SeekFrom::Start(location.offset))?;
        file.read_exact(&mut data)?;
        // The add function assumes the file position is always at the end, so reset it.
        file.seek(SeekFrom::End(0))?;

        let entry = DataEntry::new(&data, 0, self.version as u8)?;
        Ok((
            Delta {
                data: entry.delta()?,
                base: Key::new(key.name().into(), entry.delta_base().clone()),
                key: Key::new(key.name().into(), entry.node().clone()),
            },
            entry.metadata().clone(),
        ))
    }
}

impl DataStore for MutableDataPack {
    fn get(&self, key: &Key) -> Result<Vec<u8>> {
        Err(
            MutableDataPackError("DataPack doesn't support raw get(), only getdeltachain".into())
                .into(),
        )
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Vec<Delta>> {
        unimplemented!();
    }

    fn get_meta(&self, key: &Key) -> Result<Metadata> {
        let (delta_base, metadata) = self.read_entry(&key)?;
        Ok(metadata)
    }

    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        Ok(keys.iter()
            .filter(|k| self.mem_index.get(k.node()).is_none())
            .map(|k| k.clone())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::fs::File;
    use std::io::Read;
    use std::rc::Rc;

    use key::Key;
    use tempfile::tempdir;

    #[test]
    fn test_basic_creation() {
        let tempdir = tempdir().unwrap();
        let mut mutdatapack = MutableDataPack::new(tempdir.path(), 1).unwrap();
        let delta = Delta {
            data: Rc::new([0, 1, 2]),
            base: Key::new(Box::new([]), Default::default()),
            key: Key::new(Box::new([]), Default::default()),
        };
        mutdatapack.add(&delta, None).expect("add");
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
            let mut mutdatapack = MutableDataPack::new(tempdir.path(), 1).unwrap();
            let delta = Delta {
                data: Rc::new([0, 1, 2]),
                base: Key::new(Box::new([]), Default::default()),
                key: Key::new(Box::new([]), Default::default()),
            };
            mutdatapack.add(&delta, None).expect("add");
        }

        assert_eq!(fs::read_dir(tempdir.path()).unwrap().count(), 0);
    }

    #[test]
    fn test_get_meta() {
        let tempdir = tempdir().unwrap();
        let mut mutdatapack = MutableDataPack::new(tempdir.path(), 1).unwrap();
        let delta = Delta {
            data: Rc::new([0, 1, 2]),
            base: Key::new(Box::new([]), Default::default()),
            key: Key::new(Box::new([]), Node::random()),
        };
        mutdatapack.add(&delta, None).unwrap();
        let delta2 = Delta {
            data: Rc::new([0, 1, 2]),
            base: Key::new(Box::new([]), Default::default()),
            key: Key::new(Box::new([]), Node::random()),
        };
        let meta2 = Metadata {
            flags: Some(2),
            size: Some(1000),
        };
        mutdatapack.add(&delta2, Some(meta2.clone())).unwrap();

        // Requesting a default metadata
        let found_meta = mutdatapack.get_meta(&delta.key).unwrap();
        assert_eq!(found_meta, Metadata::default());

        // Requesting a specified metadata
        let found_meta = mutdatapack.get_meta(&delta2.key).unwrap();
        assert_eq!(found_meta, meta2);

        // Requesting a non-existent metadata
        let not = Key::new(Box::new([1]), Node::random());
        mutdatapack
            .get_meta(&not)
            .expect_err("expected error for non existent node");
    }

    #[test]
    fn test_get_missing() {
        let tempdir = tempdir().unwrap();
        let mut mutdatapack = MutableDataPack::new(tempdir.path(), 1).unwrap();
        let delta = Delta {
            data: Rc::new([0, 1, 2]),
            base: Key::new(Box::new([]), Default::default()),
            key: Key::new(Box::new([]), Default::default()),
        };
        mutdatapack.add(&delta, None).unwrap();

        let not = Key::new(Box::new([1]), Node::random());
        let missing = mutdatapack
            .get_missing(&vec![delta.key.clone(), not.clone()])
            .unwrap();
        assert_eq!(missing, vec![not.clone()]);
    }
}

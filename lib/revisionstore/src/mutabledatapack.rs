use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::u16;

use byteorder::{BigEndian, WriteBytesExt};
use crypto::digest::Digest;
use crypto::sha1::Sha1;
use dataindex::{DataIndex, DeltaLocation};
use datastore::{Delta, Metadata};
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
struct MutableDataPackError(&'static str);

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
            return Err(MutableDataPackError("delta name is longer than 2^16").into());
        }
        if self.version == 0 && metadata.is_some() {
            return Err(MutableDataPackError("v0 data pack cannot store metadata").into());
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
            size: compressed.len() as u64,
        };
        self.mem_index
            .insert(delta.key.node().clone(), delta_location);
        Ok(())
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
}

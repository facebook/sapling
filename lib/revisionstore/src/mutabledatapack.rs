use std::io::Write;
use std::path::{Path, PathBuf};
use std::u16;

use byteorder::{BigEndian, WriteBytesExt};
use datastore::{Delta, Metadata};
use lz4_pyframe::compress;
use tempfile::NamedTempFile;

use error::Result;

pub struct MutableDataPack {
    version: u32,
    dir: PathBuf,
    data_file: NamedTempFile,
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
    pub fn new(dir: &Path, version: u32) -> Result<Self> {
        if !dir.is_dir() {
            return Err(format_err!(
                "cannot create mutable datapack in non-directory '{:?}'",
                dir
            ));
        }

        let data_file = NamedTempFile::new_in(&dir)?;

        Ok(MutableDataPack {
            version: version,
            dir: dir.to_path_buf(),
            data_file: data_file,
        })
    }

    /// Closes the mutable datapack, returning the path of the final immutable datapack on disk.
    /// The mutable datapack is no longer usable after being closed.
    pub fn close(mut self) -> Result<PathBuf> {
        // This will be replaced later with an actual hash of the datapack contents
        let base_filename = "temp_datapack_name";

        let data_filepath = self.dir.join(base_filename);
        self.data_file.persist(&data_filepath)?;
        Ok(data_filepath)
    }

    /// Adds the given entry to the mutable datapack.
    pub fn add(&mut self, delta: &Delta, metadata: Option<Metadata>) -> Result<()> {
        if delta.key.name().len() >= u16::MAX as usize {
            return Err(MutableDataPackError("delta name is longer than 2^16").into());
        }
        if self.version == 0 && metadata.is_some() {
            return Err(MutableDataPackError("v0 data pack cannot store metadata").into());
        }

        let compressed = compress(&delta.data)?;

        let file = self.data_file.as_file_mut();
        file.write_u16::<BigEndian>(delta.key.name().len() as u16)?;
        file.write_all(delta.key.name())?;
        file.write_all(delta.key.node().as_ref())?;
        file.write_all(delta.base.node().as_ref())?;
        file.write_u64::<BigEndian>(compressed.len() as u64)?;
        file.write_all(&compressed)?;

        if self.version == 1 {
            metadata.unwrap_or_default().write(file)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use key::Key;
    use tempfile::tempdir;

    #[test]
    fn test_basic_creation() {
        let tempdir = tempdir().unwrap();
        let mut mutdatapack = MutableDataPack::new(tempdir.path(), 1).unwrap();
        let delta = Delta {
            data: Box::new([0, 1, 2]),
            base: Key::new(Box::new([]), Default::default()),
            key: Key::new(Box::new([]), Default::default()),
        };
        mutdatapack.add(&delta, None).expect("add");
        let datapackpath = mutdatapack.close().expect("close");

        assert!(datapackpath.exists());
    }

    #[test]
    fn test_basic_abort() {
        let tempdir = tempdir().unwrap();
        {
            let mut mutdatapack = MutableDataPack::new(tempdir.path(), 1).unwrap();
            let delta = Delta {
                data: Box::new([0, 1, 2]),
                base: Key::new(Box::new([]), Default::default()),
                key: Key::new(Box::new([]), Default::default()),
            };
            mutdatapack.add(&delta, None).expect("add");
        }

        assert_eq!(fs::read_dir(tempdir.path()).unwrap().count(), 0);
    }
}

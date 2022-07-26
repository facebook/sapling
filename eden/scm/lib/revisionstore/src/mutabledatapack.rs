/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::mem::replace;
use std::path::Path;
use std::path::PathBuf;
use std::u16;

use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use byteorder::BigEndian;
use byteorder::WriteBytesExt;
use lz4_pyframe::compress;
use mpatch::mpatch::get_full_text;
use parking_lot::Mutex;
use sha1::Digest;
use sha1::Sha1;
use tempfile::Builder;
use tempfile::NamedTempFile;
use thiserror::Error;
use types::HgId;
use types::Key;

use crate::dataindex::DataIndex;
use crate::dataindex::DeltaLocation;
use crate::datapack::DataEntry;
use crate::datapack::DataPackVersion;
use crate::datastore::Delta;
use crate::datastore::HgIdDataStore;
use crate::datastore::HgIdMutableDeltaStore;
use crate::datastore::Metadata;
use crate::datastore::StoreResult;
use crate::error::EmptyMutablePack;
use crate::localstore::LocalStore;
use crate::mutablepack::MutablePack;
use crate::packwriter::PackWriter;
use crate::types::StoreKey;

struct MutableDataPackInner {
    dir: PathBuf,
    data_file: PackWriter<NamedTempFile>,
    mem_index: HashMap<HgId, DeltaLocation>,
    hasher: Sha1,
}

pub struct MutableDataPack {
    dir: PathBuf,
    version: DataPackVersion,
    inner: Mutex<Option<MutableDataPackInner>>,
}

#[derive(Debug, Error)]
#[error("Mutable Data Pack Error: {0:?}")]
struct MutableDataPackError(String);

impl MutableDataPackInner {
    /// Creates a new MutableDataPack for producing datapack files.
    ///
    /// The data is written to a temporary file, and renamed to the final location
    /// when flush() is called, at which point the MutableDataPack is consumed. If
    /// flush() is not called, the temporary file is cleaned up when the object is
    /// release.
    pub fn new(dir: impl AsRef<Path>, version: DataPackVersion) -> Result<Self> {
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

        let tempfile = Builder::new().append(true).tempfile_in(&dir)?;
        let mut data_file = PackWriter::new(tempfile);
        let mut hasher = Sha1::new();
        let version_u8: u8 = version.into();
        data_file.write_u8(version_u8)?;
        hasher.update(&[version_u8]);

        Ok(Self {
            dir: dir.to_path_buf(),
            data_file,
            mem_index: HashMap::new(),
            hasher,
        })
    }

    fn read_entry(&self, key: &Key) -> Result<Option<(Delta, Metadata)>> {
        let location: &DeltaLocation = match self.mem_index.get(&key.hgid) {
            None => return Ok(None),
            Some(location) => location,
        };

        // Make sure the buffers are empty so the reads below are consistent with what is being
        // written.
        self.data_file.flush_inner()?;
        let mut file = self.data_file.get_mut();

        let mut data = Vec::with_capacity(location.size as usize);
        unsafe { data.set_len(location.size as usize) };

        file.seek(SeekFrom::Start(location.offset))?;
        file.read_exact(&mut data)?;

        let entry = DataEntry::new(&data, 0, DataPackVersion::One)?;
        Ok(Some((
            Delta {
                data: entry.delta()?,
                base: entry
                    .delta_base()
                    .map(|delta_base| Key::new(key.path.clone(), delta_base.clone())),
                key: Key::new(key.path.clone(), entry.hgid().clone()),
            },
            entry.metadata().clone(),
        )))
    }

    fn add(&mut self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        let path_slice = delta.key.path.as_byte_slice();
        if path_slice.len() >= u16::MAX as usize {
            return Err(MutableDataPackError("delta path is longer than 2^16".into()).into());
        }

        let offset = self.data_file.bytes_written();

        let compressed = compress(&delta.data)?;

        // Preallocate with approximately the size we need:
        // (namelen(2) + name + hgid(20) + hgid(20) + datalen(8) + data + metadata(~22))
        let mut buf = Vec::with_capacity(path_slice.len() + compressed.len() + 72);
        buf.write_u16::<BigEndian>(path_slice.len() as u16)?;
        buf.write_all(path_slice)?;
        buf.write_all(delta.key.hgid.as_ref())?;

        buf.write_all(
            delta
                .base
                .as_ref()
                .map_or_else(|| HgId::null_id(), |k| &k.hgid)
                .as_ref(),
        )?;
        buf.write_u64::<BigEndian>(compressed.len() as u64)?;
        buf.write_all(&compressed)?;

        metadata.write(&mut buf)?;

        self.data_file.write_all(&buf)?;
        self.hasher.update(&buf);

        let delta_location = DeltaLocation {
            delta_base: delta.base.as_ref().map(|k| k.hgid.clone()),
            offset,
            size: buf.len() as u64,
        };
        self.mem_index
            .insert(delta.key.hgid.clone(), delta_location);
        Ok(())
    }
}

impl MutableDataPack {
    pub fn new(dir: impl AsRef<Path>, version: DataPackVersion) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
            version,
            inner: Mutex::new(None),
        }
    }

    fn get_pack<'a>(
        &self,
        inner: &'a mut Option<MutableDataPackInner>,
    ) -> Result<&'a mut MutableDataPackInner> {
        if inner.is_none() {
            inner.replace(MutableDataPackInner::new(&self.dir, self.version.clone())?);
        }
        Ok(inner.as_mut().unwrap())
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        let mut guard = self.inner.lock();
        if let Some(pack) = guard.as_mut() {
            let mut chain: Vec<Delta> = Default::default();
            let mut next_key = Some(key.clone());
            while let Some(key) = next_key {
                let (delta, _metadata) = match pack.read_entry(&key) {
                    Ok(Some(entry)) => entry,
                    Ok(None) => {
                        if chain.is_empty() {
                            return Ok(None);
                        } else {
                            return Ok(Some(chain));
                        }
                    }
                    Err(e) => {
                        if chain.is_empty() {
                            return Err(e);
                        } else {
                            return Ok(Some(chain));
                        }
                    }
                };
                next_key = delta.base.clone();
                chain.push(delta);
            }
            Ok(Some(chain))
        } else {
            Ok(None)
        }
    }
}

impl HgIdMutableDeltaStore for MutableDataPack {
    /// Adds the given entry to the mutable datapack.
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        let mut guard = self.inner.lock();
        let pack = self.get_pack(&mut guard)?;
        pack.add(delta, metadata)
    }

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        let mut guard = self.inner.lock();
        let old_inner = replace(&mut *guard, None);

        if let Some(old_inner) = old_inner {
            Ok(match old_inner.close_pack()? {
                Some(pack) => Some(vec![pack]),
                None => Some(vec![]),
            })
        } else {
            Ok(None)
        }
    }
}

impl MutablePack for MutableDataPackInner {
    fn build_files(self) -> Result<(NamedTempFile, NamedTempFile, PathBuf)> {
        if self.mem_index.is_empty() {
            return Err(EmptyMutablePack.into());
        }

        let mut index_file = PackWriter::new(NamedTempFile::new_in(&self.dir)?);
        DataIndex::write(&mut index_file, &self.mem_index)?;

        Ok((
            self.data_file.into_inner()?,
            index_file.into_inner()?,
            self.dir.join(&hex::encode(self.hasher.finalize())),
        ))
    }

    fn extension(&self) -> &'static str {
        "data"
    }
}

impl MutablePack for MutableDataPack {
    fn build_files(self) -> Result<(NamedTempFile, NamedTempFile, PathBuf)> {
        let old_inner = (*self.inner.lock()).take();
        if let Some(old_inner) = old_inner {
            old_inner.build_files()
        } else {
            Err(EmptyMutablePack.into())
        }
    }

    fn extension(&self) -> &'static str {
        "data"
    }
}

impl HgIdDataStore for MutableDataPack {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        let key = match key {
            StoreKey::HgId(key) => key,
            content => return Ok(StoreResult::NotFound(content)),
        };

        let delta_chain = self.get_delta_chain(&key)?;
        let delta_chain = match delta_chain {
            Some(chain) => chain,
            None => return Ok(StoreResult::NotFound(StoreKey::HgId(key))),
        };

        let (basetext, deltas) = match delta_chain.split_last() {
            Some((base, delta)) => (base, delta),
            None => return Ok(StoreResult::NotFound(StoreKey::HgId(key))),
        };

        let deltas: Vec<&[u8]> = deltas
            .iter()
            .rev()
            .map(|delta| delta.data.as_ref())
            .collect();

        Ok(StoreResult::Found(
            get_full_text(basetext.data.as_ref(), &deltas).map_err(Error::msg)?,
        ))
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        let mut guard = self.inner.lock();
        if let Some(pack) = guard.as_mut() {
            let key = match key {
                StoreKey::HgId(key) => key,
                content => return Ok(StoreResult::NotFound(content)),
            };

            match pack.read_entry(&key)? {
                None => Ok(StoreResult::NotFound(StoreKey::HgId(key))),
                Some((_, metadata)) => Ok(StoreResult::Found(metadata)),
            }
        } else {
            Ok(StoreResult::NotFound(key))
        }
    }

    fn refresh(&self) -> Result<()> {
        Ok(())
    }
}

impl LocalStore for MutableDataPack {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let mut guard = self.inner.lock();
        if let Some(pack) = guard.as_mut() {
            Ok(keys
                .iter()
                .filter(|k| match k {
                    StoreKey::HgId(k) => pack.mem_index.get(&k.hgid).is_none(),
                    StoreKey::Content(_, _) => true,
                })
                .cloned()
                .collect())
        } else {
            Ok(keys.to_vec())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::fs::File;
    use std::io::Read;

    use minibytes::Bytes;
    use tempfile::tempdir;
    use types::testutil::*;
    use types::Key;
    use types::RepoPathBuf;

    use super::*;

    #[test]
    fn test_basic_creation() {
        let tempdir = tempdir().unwrap();
        let mutdatapack = MutableDataPack::new(tempdir.path(), DataPackVersion::One);
        let delta = Delta {
            data: Bytes::from(&[0, 1, 2][..]),
            base: None,
            key: Key::new(RepoPathBuf::new(), Default::default()),
        };
        mutdatapack.add(&delta, &Default::default()).expect("add");
        let datapackbase = mutdatapack.flush().expect("flush").unwrap()[0].clone();
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
        hasher.update(&buf);
        let hash = hex::encode(hasher.finalize());
        assert!(hash == filename_hash);
    }

    #[test]
    fn test_basic_abort() {
        let tempdir = tempdir().unwrap();
        {
            let mutdatapack = MutableDataPack::new(tempdir.path(), DataPackVersion::One);
            let delta = Delta {
                data: Bytes::from(&[0, 1, 2][..]),
                base: None,
                key: Key::new(RepoPathBuf::new(), Default::default()),
            };
            mutdatapack.add(&delta, &Default::default()).expect("add");
        }

        assert_eq!(fs::read_dir(tempdir.path()).unwrap().count(), 0);
    }

    #[test]
    fn test_get_delta_chain() {
        let tempdir = tempdir().unwrap();
        let mutdatapack = MutableDataPack::new(tempdir.path(), DataPackVersion::One);
        let delta = Delta {
            data: Bytes::from(&[0, 1, 2][..]),
            base: None,
            key: Key::new(RepoPathBuf::new(), hgid("1")),
        };
        mutdatapack.add(&delta, &Default::default()).unwrap();
        let delta2 = Delta {
            data: Bytes::from(&[0, 1, 2][..]),
            base: Some(Key::new(RepoPathBuf::new(), delta.key.hgid.clone())),
            key: Key::new(RepoPathBuf::new(), hgid("2")),
        };
        mutdatapack.add(&delta2, &Default::default()).unwrap();

        let chain = mutdatapack.get_delta_chain(&delta.key).unwrap();
        assert_eq!(&vec![delta.clone()], &chain.unwrap());

        let chain = mutdatapack.get_delta_chain(&delta2.key).unwrap();
        assert_eq!(&vec![delta2.clone(), delta.clone()], &chain.unwrap());
    }

    #[test]
    fn test_get_partial_delta_chain() -> Result<()> {
        let tempdir = tempdir()?;
        let mutdatapack = MutableDataPack::new(tempdir.path(), DataPackVersion::One);

        let delta = Delta {
            data: Bytes::from(&[0, 1, 2][..]),
            base: Some(key("a", "1")),
            key: key("a", "2"),
        };

        mutdatapack.add(&delta, &Default::default())?;
        let chain = mutdatapack.get_delta_chain(&delta.key)?.unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain.get(0), Some(&delta));

        Ok(())
    }

    #[test]
    fn test_get_meta() {
        let tempdir = tempdir().unwrap();

        let mutdatapack = MutableDataPack::new(tempdir.path(), DataPackVersion::One);
        let delta = Delta {
            data: Bytes::from(&[0, 1, 2][..]),
            base: None,
            key: Key::new(RepoPathBuf::new(), hgid("1")),
        };
        mutdatapack.add(&delta, &Default::default()).unwrap();
        let delta2 = Delta {
            data: Bytes::from(&[0, 1, 2][..]),
            base: None,
            key: Key::new(RepoPathBuf::new(), hgid("2")),
        };
        let meta2 = Metadata {
            flags: Some(2),
            size: Some(1000),
        };
        mutdatapack.add(&delta2, &meta2).unwrap();

        // Requesting a default metadata
        let found_meta = mutdatapack.get_meta(StoreKey::hgid(delta.key)).unwrap();
        assert_eq!(found_meta, StoreResult::Found(Metadata::default()));

        // Requesting a specified metadata
        let found_meta = mutdatapack.get_meta(StoreKey::hgid(delta2.key)).unwrap();
        assert_eq!(found_meta, StoreResult::Found(meta2));

        // Requesting a non-existent metadata
        let not = StoreKey::hgid(key("not", "10000"));
        assert_eq!(
            mutdatapack.get_meta(not.clone()).unwrap(),
            StoreResult::NotFound(not)
        );
    }

    #[test]
    fn test_get_missing() {
        let tempdir = tempdir().unwrap();

        let mutdatapack = MutableDataPack::new(tempdir.path(), DataPackVersion::One);
        let delta = Delta {
            data: Bytes::from(&[0, 1, 2][..]),
            base: None,
            key: Key::new(RepoPathBuf::new(), Default::default()),
        };
        mutdatapack.add(&delta, &Default::default()).unwrap();

        let not = key("not", "10000");
        let missing = mutdatapack
            .get_missing(&vec![StoreKey::from(delta.key), StoreKey::from(&not)])
            .unwrap();
        assert_eq!(missing, vec![StoreKey::from(not)]);
    }

    #[test]
    fn test_empty() {
        let tempdir = tempdir().unwrap();

        let mutdatapack = MutableDataPack::new(tempdir.path(), DataPackVersion::One);
        assert_eq!(mutdatapack.flush().unwrap(), None);
        drop(mutdatapack);
        assert_eq!(fs::read_dir(tempdir.path()).unwrap().count(), 0);
    }
}

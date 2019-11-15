/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Classes for constructing and serializing a datapack file and index.
//!
//! A datapack is a pair of files that contain the revision contents for various
//! file revisions in Mercurial. It contains only revision contents (like file
//! contents), not any history information.
//!
//! It consists of two files, with the following format. All bytes are in
//! network byte order (big endian).
//!
//! ```text
//!
//! .datapack
//!     The pack itself is a series of revision deltas with some basic header
//!     information on each. A revision delta may be a fulltext, represented by
//!     a deltabasenode equal to the nullid.
//!
//!     datapack = <version: 1 byte>
//!                [<revision>,...]
//!     revision = <filename len: 2 byte unsigned int>
//!                <filename>
//!                <hgid: 20 byte>
//!                <deltabasenode: 20 byte>
//!                <delta len: 8 byte unsigned int>
//!                <delta>
//!                <metadata-list len: 4 byte unsigned int> [1]
//!                <metadata-list>                          [1]
//!     metadata-list = [<metadata-item>, ...]
//!     metadata-item = <metadata-key: 1 byte>
//!                     <metadata-value len: 2 byte unsigned>
//!                     <metadata-value>
//!
//!     metadata-key could be METAKEYFLAG or METAKEYSIZE or other single byte
//!     value in the future.
//!
//! .dataidx
//!     The index file consists of two parts, the fanout and the index.
//!
//!     The index is a list of index entries, sorted by hgid (one per revision
//!     in the pack). Each entry has:
//!
//!     - hgid (The 20 byte hgid of the entry; i.e. the commit hash, file hgid
//!             hash, etc)
//!     - deltabase index offset (The location in the index of the deltabase for
//!                               this entry. The deltabase is the next delta in
//!                               the chain, with the chain eventually
//!                               terminating in a full-text, represented by a
//!                               deltabase offset of -1. This lets us compute
//!                               delta chains from the index, then do
//!                               sequential reads from the pack if the revision
//!                               are nearby on disk.)
//!     - pack entry offset (The location of this entry in the datapack)
//!     - pack content size (The on-disk length of this entry's pack data)
//!
//!     The fanout is a quick lookup table to reduce the number of steps for
//!     bisecting the index. It is a series of 4 byte pointers to positions
//!     within the index. It has 2^16 entries, which corresponds to hash
//!     prefixes [0000, 0001,..., FFFE, FFFF]. Example: the pointer in slot
//!     4F0A points to the index position of the first revision whose hgid
//!     starts with 4F0A. This saves log(2^16)=16 bisect steps.
//!
//!     dataidx = <version: 1 byte>
//!               <config: 1 byte>
//!               <fanouttable>
//!               <index>
//!     fanouttable = [<index offset: 4 byte unsigned int>,...] (2^8 or 2^16 entries)
//!     index = [<index entry>,...]
//!     indexentry = <hgid: 20 byte>
//!                  <deltabase location: 4 byte signed int>
//!                  <pack entry offset: 8 byte unsigned int>
//!                  <pack entry size: 8 byte unsigned int>
//!
//! ```
//! [1]: new in version 1.

use std::{
    cell::RefCell,
    fmt,
    fs::File,
    io::{Cursor, Read},
    mem::{drop, replace},
    path::{Path, PathBuf},
    sync::Arc,
};

use byteorder::{BigEndian, ReadBytesExt};
use bytes::Bytes;
use failure::{format_err, Fallible as Result};
use memmap::{Mmap, MmapOptions};
use thiserror::Error;

use lz4_pyframe::decompress;
use types::{HgId, Key, RepoPath};
use util::path::remove_file;

use crate::dataindex::{DataIndex, DeltaBaseOffset};
use crate::datastore::{DataStore, Delta, Metadata};
use crate::localstore::LocalStore;
use crate::repack::{Repackable, ToKeys};
use crate::sliceext::SliceExt;

#[derive(Debug, Error)]
#[error("Datapack Error: {0:?}")]
struct DataPackError(String);

#[derive(Clone, PartialEq)]
pub enum DataPackVersion {
    Zero,
    One,
}

pub struct DataPack {
    mmap: Mmap,
    version: DataPackVersion,
    index: DataIndex,
    base_path: Arc<PathBuf>,
    pack_path: PathBuf,
    index_path: PathBuf,
}

pub struct DataEntry<'a> {
    offset: u64,
    filename: &'a RepoPath,
    hgid: HgId,
    delta_base: Option<HgId>,
    compressed_data: &'a [u8],
    data: RefCell<Option<Bytes>>,
    metadata: Metadata,
    next_offset: u64,
}

impl DataPackVersion {
    fn new(value: u8) -> Result<Self> {
        match value {
            0 => Ok(DataPackVersion::Zero),
            1 => Ok(DataPackVersion::One),
            _ => {
                Err(DataPackError(format!("invalid datapack version number '{:?}'", value)).into())
            }
        }
    }
}

impl From<DataPackVersion> for u8 {
    fn from(version: DataPackVersion) -> u8 {
        match version {
            DataPackVersion::Zero => 0,
            DataPackVersion::One => 1,
        }
    }
}

impl<'a> DataEntry<'a> {
    pub fn new(buf: &'a [u8], offset: u64, version: DataPackVersion) -> Result<Self> {
        let mut cur = Cursor::new(buf);
        cur.set_position(offset);

        // Filename
        let filename_len = cur.read_u16::<BigEndian>()? as u64;
        let filename_slice =
            buf.get_err(cur.position() as usize..(cur.position() + filename_len) as usize)?;
        let filename = RepoPath::from_utf8(filename_slice)?;
        let cur_pos = cur.position();
        cur.set_position(cur_pos + filename_len);

        // HgId
        let mut hgid_buf: [u8; 20] = Default::default();
        cur.read_exact(&mut hgid_buf)?;
        let hgid = HgId::from(&hgid_buf);

        // Delta
        cur.read_exact(&mut hgid_buf)?;
        let delta_base = HgId::from(&hgid_buf);
        let delta_base = if delta_base.is_null() {
            None
        } else {
            Some(delta_base)
        };

        let delta_len = cur.read_u64::<BigEndian>()?;
        let compressed_data =
            buf.get_err(cur.position() as usize..(cur.position() + delta_len) as usize)?;
        let data = RefCell::new(None);
        let cur_pos = cur.position();
        cur.set_position(cur_pos + delta_len);

        // Metadata
        let mut metadata = Metadata {
            flags: None,
            size: None,
        };
        if version == DataPackVersion::One {
            metadata = Metadata::read(&mut cur)?;
        }

        let next_offset = cur.position();

        Ok(DataEntry {
            offset,
            filename,
            hgid,
            delta_base,
            compressed_data,
            data,
            metadata,
            next_offset,
        })
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn filename(&self) -> &RepoPath {
        self.filename
    }

    pub fn hgid(&self) -> &HgId {
        &self.hgid
    }

    pub fn delta_base(&self) -> &Option<HgId> {
        &self.delta_base
    }

    pub fn delta(&self) -> Result<Bytes> {
        let mut cell = self.data.borrow_mut();
        if cell.is_none() {
            *cell = Some(decompress(&self.compressed_data)?.into());
        }

        Ok(cell.as_ref().unwrap().clone())
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
}

impl<'a> fmt::Debug for DataEntry<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let delta = self
            .delta()
            .unwrap_or_else(|e| Bytes::from(format!("{:?}", e).as_bytes()));
        write!(
            f,
            "DataEntry {{\n  offset: {:?}\n  filename: {:?}\n  \
             hgid: {:?}\n  delta_base: {:?}\n  compressed_len: {:?}\n  \
             data_len: {:?}\n  data: {:?}\n  metadata: N/A\n}}",
            self.offset,
            self.filename,
            self.hgid,
            self.delta_base,
            self.compressed_data.len(),
            delta.len(),
            delta.iter().map(|b| *b as char).collect::<String>(),
        )
    }
}

impl DataPack {
    pub fn new(path: &Path) -> Result<Self> {
        let base_path = PathBuf::from(path);
        let pack_path = path.with_extension("datapack");
        let file = File::open(&pack_path)?;
        let len = file.metadata()?.len();
        if len < 1 {
            return Err(format_err!(
                "empty datapack '{:?}' is invalid",
                path.to_str().unwrap_or("<unknown>")
            ));
        }

        let mmap = unsafe { MmapOptions::new().len(len as usize).map(&file)? };
        let version = DataPackVersion::new(mmap[0])?;
        let index_path = path.with_extension("dataidx");
        Ok(DataPack {
            mmap,
            version,
            index: DataIndex::new(&index_path)?,
            base_path: Arc::new(base_path),
            pack_path,
            index_path,
        })
    }

    pub fn len(&self) -> usize {
        self.mmap.len()
    }

    pub fn read_entry(&self, offset: u64) -> Result<DataEntry> {
        DataEntry::new(self.mmap.as_ref(), offset, self.version.clone())
    }

    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    pub fn pack_path(&self) -> &Path {
        &self.pack_path
    }

    pub fn index_path(&self) -> &Path {
        &self.index_path
    }
}

impl DataStore for DataPack {
    fn get(&self, _key: &Key) -> Result<Option<Vec<u8>>> {
        Err(format_err!(
            "DataPack doesn't support raw get(), only getdeltachain"
        ))
    }

    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        let entry = match self.index.get_entry(&key.hgid)? {
            None => return Ok(None),
            Some(entry) => entry,
        };
        let data_entry = self.read_entry(entry.pack_entry_offset())?;

        Ok(Some(Delta {
            data: data_entry.delta()?,
            base: data_entry
                .delta_base()
                .map(|delta_base| Key::new(key.path.clone(), delta_base.clone())),
            key: Key::new(key.path.clone(), data_entry.hgid().clone()),
        }))
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        let mut chain: Vec<Delta> = Default::default();
        let mut next_entry = match self.index.get_entry(&key.hgid)? {
            None => return Ok(None),
            Some(entry) => entry,
        };
        loop {
            let data_entry = self.read_entry(next_entry.pack_entry_offset())?;
            chain.push(Delta {
                data: data_entry.delta()?,
                base: data_entry
                    .delta_base()
                    .map(|delta_base| Key::new(data_entry.filename.to_owned(), delta_base.clone())),
                key: Key::new(data_entry.filename.to_owned(), data_entry.hgid().clone()),
            });

            if let DeltaBaseOffset::Offset(offset) = next_entry.delta_base_offset() {
                next_entry = self.index.read_entry(offset as usize)?;
            } else {
                break;
            }
        }

        Ok(Some(chain))
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        let index_entry = match self.index.get_entry(&key.hgid)? {
            None => return Ok(None),
            Some(entry) => entry,
        };
        Ok(Some(
            self.read_entry(index_entry.pack_entry_offset())?.metadata,
        ))
    }
}

impl LocalStore for DataPack {
    fn from_path(path: &Path) -> Result<Self> {
        DataPack::new(path)
    }

    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        Ok(keys
            .iter()
            .filter(|k| match self.index.get_entry(&k.hgid) {
                Ok(None) | Err(_) => true,
                Ok(Some(_)) => false,
            })
            .map(|k| k.clone())
            .collect())
    }
}

impl ToKeys for DataPack {
    fn to_keys(&self) -> Vec<Result<Key>> {
        DataPackIterator::new(self).collect()
    }
}

impl Repackable for DataPack {
    fn delete(mut self) -> Result<()> {
        // On some platforms, removing a file can fail if it's still opened or mapped, let's make
        // sure we close and unmap them before deletion.
        let pack_path = replace(&mut self.pack_path, Default::default());
        let index_path = replace(&mut self.index_path, Default::default());
        drop(self);

        let result1 = remove_file(&pack_path);
        let result2 = remove_file(&index_path);
        // Only check for errors after both have run. That way if pack_path doesn't exist,
        // index_path is still deleted.
        result1?;
        result2?;
        Ok(())
    }
}

struct DataPackIterator<'a> {
    pack: &'a DataPack,
    offset: u64,
}

impl<'a> DataPackIterator<'a> {
    pub fn new(pack: &'a DataPack) -> Self {
        DataPackIterator {
            pack,
            offset: 1, // Start after the header byte
        }
    }
}

impl<'a> Iterator for DataPackIterator<'a> {
    type Item = Result<Key>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset as usize >= self.pack.len() {
            return None;
        }
        let entry = self.pack.read_entry(self.offset);
        Some(match entry {
            Ok(ref e) => {
                self.offset = e.next_offset;
                Ok(Key::new(e.filename.to_owned(), e.hgid))
            }
            Err(e) => {
                // The entry is corrupted, and we have no way to know where the next one is
                // located, let's forcibly stop the iteration.
                self.offset = self.pack.len() as u64;
                Err(e)
            }
        })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    use std::rc::Rc;

    use quickcheck::quickcheck;
    use tempfile::TempDir;

    use types::testutil::*;

    use crate::datastore::{Delta, Metadata, MutableDeltaStore};
    use crate::mutabledatapack::MutableDataPack;

    pub fn make_datapack(tempdir: &TempDir, deltas: &Vec<(Delta, Metadata)>) -> DataPack {
        let mutdatapack = MutableDataPack::new(tempdir.path(), DataPackVersion::One).unwrap();
        for (delta, metadata) in deltas.iter() {
            mutdatapack.add(delta, metadata).unwrap();
        }

        let path = mutdatapack.flush().unwrap().unwrap();

        DataPack::new(&path).unwrap()
    }

    #[test]
    fn test_get_missing() {
        let tempdir = TempDir::new().unwrap();

        let revisions = vec![(
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: Some(key("a", "1")),
                key: key("a", "2"),
            },
            Default::default(),
        )];
        let pack = make_datapack(&tempdir, &revisions);
        for &(ref delta, ref _metadata) in revisions.iter() {
            let missing = pack.get_missing(&[delta.key.clone()]).unwrap();
            assert_eq!(missing.len(), 0);
        }

        let not = key("b", "3");
        let missing = pack.get_missing(&vec![not.clone()]).unwrap();
        assert_eq!(missing, vec![not.clone()]);
    }

    #[test]
    fn test_get_meta() {
        let tempdir = TempDir::new().unwrap();

        let revisions = vec![
            (
                Delta {
                    data: Bytes::from(&[1, 2, 3, 4][..]),
                    base: Some(key("a", "1")),
                    key: key("a", "2"),
                },
                Default::default(),
            ),
            (
                Delta {
                    data: Bytes::from(&[1, 2, 3, 4][..]),
                    base: Some(key("a", "3")),
                    key: key("a", "4"),
                },
                Metadata {
                    size: Some(1000),
                    flags: Some(7),
                },
            ),
        ];

        let pack = make_datapack(&tempdir, &revisions);
        for &(ref delta, ref metadata) in revisions.iter() {
            let meta = pack.get_meta(&delta.key).unwrap().unwrap();
            assert_eq!(&meta, metadata);
        }
    }

    #[test]
    fn test_get_delta_chain_single() {
        let tempdir = TempDir::new().unwrap();

        let revisions = vec![
            (
                Delta {
                    data: Bytes::from(&[1, 2, 3, 4][..]),
                    base: Some(key("a", "1")),
                    key: key("a", "2"),
                },
                Default::default(),
            ),
            (
                Delta {
                    data: Bytes::from(&[1, 2, 3, 4][..]),
                    base: Some(key("a", "3")),
                    key: key("a", "4"),
                },
                Default::default(),
            ),
        ];

        let pack = make_datapack(&tempdir, &revisions);
        for &(ref delta, ref _metadata) in revisions.iter() {
            let chain = pack.get_delta_chain(&delta.key).unwrap().unwrap();
            assert_eq!(chain[0], *delta);
        }
    }

    #[test]
    fn test_get_delta() {
        let tempdir = TempDir::new().unwrap();

        let revisions = vec![
            (
                Delta {
                    data: Bytes::from(&[1, 2, 3, 4][..]),
                    base: Some(key("a", "1")),
                    key: key("a", "2"),
                },
                Default::default(),
            ),
            (
                Delta {
                    data: Bytes::from(&[1, 2, 3, 4][..]),
                    base: Some(key("a", "3")),
                    key: key("a", "4"),
                },
                Default::default(),
            ),
        ];

        let pack = make_datapack(&tempdir, &revisions);
        for &(ref expected_delta, _) in revisions.iter() {
            let delta = pack.get_delta(&expected_delta.key).unwrap().unwrap();
            assert_eq!(expected_delta, &delta);
        }
    }

    #[test]
    fn test_get_delta_chain_multiple() {
        let tempdir = TempDir::new().unwrap();

        let mut revisions = vec![(
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: Some(key("a", "1")),
                key: key("a", "2"),
            },
            Default::default(),
        )];
        let base0 = revisions[0].0.key.clone();
        revisions.push((
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: Some(base0),
                key: key("a", "3"),
            },
            Default::default(),
        ));
        let base1 = revisions[1].0.key.clone();
        revisions.push((
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: Some(base1),
                key: key("a", "4"),
            },
            Default::default(),
        ));

        let pack = make_datapack(&tempdir, &revisions);

        let chains = [
            vec![revisions[0].0.clone()],
            vec![revisions[1].0.clone(), revisions[0].0.clone()],
            vec![
                revisions[2].0.clone(),
                revisions[1].0.clone(),
                revisions[0].0.clone(),
            ],
        ];

        for i in 0..2 {
            let chain = pack.get_delta_chain(&revisions[i].0.key).unwrap().unwrap();
            assert_eq!(&chains[i], &chain);
        }
    }

    #[test]
    fn test_iter() {
        let tempdir = TempDir::new().unwrap();

        let revisions = vec![
            (
                Delta {
                    data: Bytes::from(&[1, 2, 3, 4][..]),
                    base: Some(key("a", "1")),
                    key: key("a", "2"),
                },
                Default::default(),
            ),
            (
                Delta {
                    data: Bytes::from(&[1, 2, 3, 4][..]),
                    base: Some(key("a", "3")),
                    key: key("a", "4"),
                },
                Default::default(),
            ),
        ];

        let pack = make_datapack(&tempdir, &revisions);
        assert_eq!(
            pack.to_keys()
                .into_iter()
                .collect::<Result<Vec<Key>>>()
                .unwrap(),
            revisions
                .iter()
                .map(|d| d.0.key.clone())
                .collect::<Vec<Key>>()
        );
    }

    #[test]
    fn test_delete() {
        let tempdir = TempDir::new().unwrap();

        let revisions = vec![(
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: None,
                key: key("a", "1"),
            },
            Default::default(),
        )];

        let pack = make_datapack(&tempdir, &revisions);
        assert_eq!(
            tempdir.path().read_dir().unwrap().collect::<Vec<_>>().len(),
            2
        );
        pack.delete().unwrap();
        assert_eq!(
            tempdir.path().read_dir().unwrap().collect::<Vec<_>>().len(),
            0
        );
    }

    #[test]
    fn test_delete_while_open() {
        let tempdir = TempDir::new().unwrap();

        let revisions = vec![(
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: None,
                key: key("a", "1"),
            },
            Default::default(),
        )];

        let pack = make_datapack(&tempdir, &revisions);
        let pack2 = DataPack::new(pack.base_path()).unwrap();
        assert!(pack.delete().is_ok());
        assert!(!pack2.pack_path().exists());
        assert!(!pack2.index_path().exists());
    }

    #[test]
    fn test_rc() {
        let tempdir = TempDir::new().unwrap();

        let revisions = vec![(
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: None,
                key: key("a", "1"),
            },
            Default::default(),
        )];

        let pack = Rc::new(make_datapack(&tempdir, &revisions));
        let delta = pack.get_delta(&revisions[0].0.key).unwrap().unwrap();
        assert_eq!(delta.data, revisions[0].0.data);
    }

    quickcheck! {
        fn test_iter_quickcheck(keys: Vec<(Vec<u8>, Key)>) -> bool {
            if keys.is_empty() {
                return true;
            }

            let tempdir = TempDir::new().unwrap();

            let mut revisions = vec![];
            for (data, key) in keys {
                revisions.push((
                    Delta {
                        data: data.into(),
                        base: None,
                        key: key.clone(),
                    },
                    Default::default(),
                ));
            }

            let pack = make_datapack(&tempdir, &revisions);
            let same = pack.to_keys().into_iter().collect::<Result<Vec<Key>>>().unwrap()
                == revisions
                    .iter()
                    .map(|d| d.0.key.clone())
                    .collect::<Vec<Key>>();
            same
        }
    }
}

// Copyright Facebook, Inc. 2018
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
//!                <node: 20 byte>
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
//!     The index is a list of index entries, sorted by node (one per revision
//!     in the pack). Each entry has:
//!
//!     - node (The 20 byte node of the entry; i.e. the commit hash, file node
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
//!     4F0A points to the index position of the first revision whose node
//!     starts with 4F0A. This saves log(2^16)=16 bisect steps.
//!
//!     dataidx = <version: 1 byte>
//!               <config: 1 byte>
//!               <fanouttable>
//!               <index>
//!     fanouttable = [<index offset: 4 byte unsigned int>,...] (2^8 or 2^16 entries)
//!     index = [<index entry>,...]
//!     indexentry = <node: 20 byte>
//!                  <deltabase location: 4 byte signed int>
//!                  <pack entry offset: 8 byte unsigned int>
//!                  <pack entry size: 8 byte unsigned int>
//!
//! ```
//! [1]: new in version 1.
use memmap::{Mmap, MmapOptions};
use std::fs::File;
use std::path::Path;

use dataindex::DataIndex;
use error::Result;

pub struct DataPack {
    mmap: Mmap,
    version: u8,
    index: DataIndex,
}

impl DataPack {
    pub fn new(path: &Path) -> Result<Self> {
        let path = path.with_extension("datapack");
        let file = File::open(&path)?;
        let len = file.metadata()?.len();
        if len < 1 {
            return Err(format_err!(
                "empty datapack '{:?}' is invalid",
                path.to_str().unwrap_or("<unknown>")
            ));
        }

        let mmap = unsafe { MmapOptions::new().len(len as usize).map(&file)? };
        let version = mmap[0];
        let index_path = path.with_extension("dataidx");
        Ok(DataPack {
            mmap: mmap,
            version: version,
            index: DataIndex::new(&index_path)?,
        })
    }

    pub fn len(&self) -> usize {
        self.mmap.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datastore::{Delta, Metadata};
    use mutabledatapack::MutableDataPack;
    use tempfile::TempDir;

    fn make_pack(tempdir: &TempDir, deltas: &Vec<(Delta, Option<Metadata>)>) -> DataPack {
        let mut mutdatapack = MutableDataPack::new(tempdir.path(), 1).unwrap();
        for &(ref delta, ref metadata) in deltas.iter() {
            mutdatapack.add(&delta, metadata.clone()).unwrap();
        }

        let path = mutdatapack.close().unwrap();

        DataPack::new(&path).unwrap()
    }

    #[test]
    fn test_empty() {
        let tempdir = TempDir::new().unwrap();
        let pack = make_pack(&tempdir, &vec![]);
        assert!(pack.len() > 0);
    }
}

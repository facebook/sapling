// Copyright Facebook, Inc. 2019

use std::path::{Path, PathBuf};

use bytes::Bytes;
use failure::Fallible;

use revisionstore::{
    DataPackVersion, Delta, HistoryPackVersion, Metadata, MutableDataPack, MutableDeltaStore,
    MutableHistoryPack,
};
use types::{HistoryEntry, Key};

/// Create a new datapack in the given directory, and populate it with the file
/// contents provided by the given iterator. Each Delta written to the datapack is
/// assumed to contain the full text of the corresponding file, and as a result the
/// base revision for each file is always specified as None.
pub fn write_datapack(
    pack_dir: impl AsRef<Path>,
    files: impl IntoIterator<Item = (Key, Bytes)>,
) -> Fallible<PathBuf> {
    let mut datapack = MutableDataPack::new(pack_dir, DataPackVersion::One)?;
    for (key, data) in files {
        let metadata = Metadata {
            size: Some(data.len() as u64),
            flags: None,
        };
        let delta = Delta {
            data,
            base: None,
            key,
        };
        datapack.add(&delta, &metadata)?;
    }
    datapack.close()
}

/// Create a new historypack in the given directory, and populate it
/// with the given history entries.
pub fn write_historypack(
    pack_dir: impl AsRef<Path>,
    entries: impl IntoIterator<Item = HistoryEntry>,
) -> Fallible<PathBuf> {
    let mut historypack = MutableHistoryPack::new(pack_dir, HistoryPackVersion::One)?;
    historypack.add_entries(entries)?;
    historypack.close()
}

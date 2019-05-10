// Copyright Facebook, Inc. 2019

use std::path::{Path, PathBuf};

use bytes::Bytes;
use failure::Fallible;

use revisionstore::{
    Delta, HistoryPackVersion, Metadata, MutableDeltaStore, MutableHistoryPack, MutableHistoryStore,
};
use types::{HistoryEntry, Key};

/// Populate the store with the file contents provided by the given iterator. Each Delta written to
/// the store is assumed to contain the full text of the corresponding file, and as a result the
/// base revision for each file is always specified as None.
///
/// Flushing the store for the written content to be visible is the responsability of the caller.
pub fn write_to_deltastore(
    store: &mut MutableDeltaStore,
    files: impl IntoIterator<Item = (Key, Bytes)>,
) -> Fallible<()> {
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
        store.add(&delta, &metadata)?;
    }

    Ok(())
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

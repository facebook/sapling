// Copyright Facebook, Inc. 2019

use bytes::Bytes;
use failure::Fallible;

use revisionstore::{Delta, Metadata, MutableDeltaStore, MutableHistoryStore};
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

/// Populate the store will the history entries provided by the given iterator.
///
/// Flushing the store for the written content to be visible is the responsability of the caller.
pub fn write_to_historystore(
    store: &mut MutableHistoryStore,
    entries: impl IntoIterator<Item = HistoryEntry>,
) -> Fallible<()> {
    for entry in entries {
        store.add_entry(&entry)?;
    }

    Ok(())
}

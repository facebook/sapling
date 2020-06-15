/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::Error;
use futures::future::poll_fn;
use tokio::prelude::*;
use tokio_threadpool::blocking;

use revisionstore::{DataPackVersion, MutableDataPack};

use crate::asyncmutabledeltastore::AsyncHgIdMutableDeltaStore;

pub type AsyncMutableDataPack = AsyncHgIdMutableDeltaStore<MutableDataPack>;

impl AsyncMutableDataPack {
    pub fn new(
        dir: PathBuf,
        version: DataPackVersion,
    ) -> impl Future<Item = Self, Error = Error> + Send + 'static {
        poll_fn(move || blocking(|| MutableDataPack::new(&dir, version.clone())))
            .from_err()
            .and_then(|res| res)
            .map(move |datapack| AsyncHgIdMutableDeltaStore::new_(datapack))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use bytes::Bytes;
    use tempfile::tempdir;
    use tokio::runtime::Runtime;

    use revisionstore::{DataPack, Delta, HgIdDataStore};
    use types::{Key, RepoPathBuf};

    #[test]
    fn test_add() {
        let tempdir = tempdir().unwrap();

        let mutabledatapack =
            AsyncMutableDataPack::new(tempdir.path().to_path_buf(), DataPackVersion::One);

        let delta = Delta {
            data: Bytes::from(&[0, 1, 2][..]),
            base: None,
            key: Key::new(RepoPathBuf::new(), Default::default()),
        };

        let cloned_delta = delta.clone();
        let work =
            mutabledatapack.and_then(move |datapack| datapack.add(&delta, &Default::default()));
        let work = work.and_then(move |datapack| datapack.close());

        let mut runtime = Runtime::new().unwrap();
        let datapackbase = runtime.block_on(work).unwrap().unwrap();
        let path = datapackbase.with_extension("datapack");

        let pack = DataPack::new(&path).unwrap();
        let stored = pack.get(&cloned_delta.key).unwrap();
        assert_eq!(stored.as_deref(), Some(cloned_delta.data.as_ref()));
    }

    #[test]
    fn test_empty_close() {
        let tempdir = tempdir().unwrap();

        let mutabledatapack =
            AsyncMutableDataPack::new(tempdir.path().to_path_buf(), DataPackVersion::One);
        let work = mutabledatapack.and_then(move |datapack| datapack.close());
        let mut runtime = Runtime::new().unwrap();

        let datapackbase = runtime.block_on(work).unwrap();
        assert_eq!(datapackbase, None);
    }
}

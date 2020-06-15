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

use revisionstore::DataPack;

use crate::asyncdatastore::AsyncHgIdDataStore;

pub type AsyncDataPack = AsyncHgIdDataStore<DataPack>;

impl AsyncDataPack {
    /// Opens the datapack at `path`.
    pub fn new(path: PathBuf) -> impl Future<Item = AsyncDataPack, Error = Error> + Send + 'static {
        poll_fn(move || blocking(|| DataPack::new(&path)))
            .from_err()
            .and_then(|res| res)
            .map(move |datapack| AsyncHgIdDataStore::new_(datapack))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;
    use tokio::runtime::Runtime;

    use revisionstore::{
        testutil::*, DataPackVersion, Delta, HgIdMutableDeltaStore, Metadata, MutableDataPack,
    };
    use types::testutil::*;

    fn make_datapack(
        tempdir: &TempDir,
        deltas: &[(Delta, Metadata)],
    ) -> impl Future<Item = AsyncDataPack, Error = Error> + 'static {
        let mutdatapack = MutableDataPack::new(tempdir.path(), DataPackVersion::One).unwrap();
        for (delta, metadata) in deltas.iter() {
            mutdatapack.add(delta, metadata).unwrap();
        }

        let path = mutdatapack.flush().unwrap().unwrap();

        AsyncDataPack::new(path)
    }

    #[test]
    fn test_one_delta() {
        let tempdir = TempDir::new().unwrap();

        let my_delta = delta("1234", None, key("a", "2"));
        let revisions = vec![(my_delta.clone(), Default::default())];

        let work = make_datapack(&tempdir, &revisions)
            .and_then(move |datapack| datapack.get(&key("a", "2")));

        let mut runtime = Runtime::new().unwrap();
        let ret_data = runtime.block_on(work).unwrap();
        assert_eq!(Some(my_delta.data.as_ref()), ret_data.as_deref());
    }

    #[test]
    fn test_multiple_delta() {
        let tempdir = TempDir::new().unwrap();

        let delta1 = delta("1234", None, key("a", "2"));
        let delta2 = delta("1234", None, key("a", "4"));
        let revisions = vec![
            (delta1.clone(), Default::default()),
            (delta2.clone(), Default::default()),
        ];

        let work = make_datapack(&tempdir, &revisions);
        let work = work.and_then(move |datapack| {
            let data = datapack.get(&key("a", "2"));
            data.and_then(move |data| {
                assert_eq!(data.as_deref(), Some(delta1.data.as_ref()));
                datapack.get(&key("a", "4"))
            })
        });

        let mut runtime = Runtime::new().unwrap();
        let ret_data = runtime.block_on(work).unwrap();
        assert_eq!(Some(delta2.data.as_ref()), ret_data.as_deref());
    }
}

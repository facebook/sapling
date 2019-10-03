// Copyright 2019 Facebook, Inc.

use std::path::PathBuf;

use failure::Error;
use futures::future::poll_fn;
use tokio::prelude::*;
use tokio_threadpool::blocking;

use revisionstore::DataPack;

use crate::asyncdatastore::AsyncDataStore;

pub type AsyncDataPack = AsyncDataStore<DataPack>;

impl AsyncDataPack {
    /// Opens the datapack at `path`.
    pub fn new(path: PathBuf) -> impl Future<Item = AsyncDataPack, Error = Error> + Send + 'static {
        poll_fn({ move || blocking(|| DataPack::new(&path)) })
            .from_err()
            .and_then(|res| res)
            .map(move |datapack| AsyncDataStore::new_(datapack))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;
    use tokio::runtime::Runtime;

    use revisionstore::{
        testutil::*, DataPackVersion, Delta, Metadata, MutableDataPack, MutableDeltaStore,
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

        let my_delta = delta("1234", Some(key("a", "1")), key("a", "2"));
        let revisions = vec![(my_delta.clone(), Default::default())];

        let work = make_datapack(&tempdir, &revisions)
            .and_then(move |datapack| datapack.get_delta(&key("a", "2")));

        let mut runtime = Runtime::new().unwrap();
        let ret_delta = runtime.block_on(work).unwrap().unwrap();
        assert_eq!(my_delta, ret_delta);
    }

    #[test]
    fn test_multiple_delta() {
        let tempdir = TempDir::new().unwrap();

        let delta1 = delta("1234", Some(key("a", "1")), key("a", "2"));
        let delta2 = delta("1234", Some(key("a", "3")), key("a", "4"));
        let revisions = vec![
            (delta1.clone(), Default::default()),
            (delta2.clone(), Default::default()),
        ];

        let work = make_datapack(&tempdir, &revisions);
        let work = work.and_then(move |datapack| {
            let delta = datapack.get_delta(&key("a", "2"));
            delta.and_then(move |delta| {
                assert_eq!(delta.unwrap(), delta1);
                datapack.get_delta(&key("a", "4"))
            })
        });

        let mut runtime = Runtime::new().unwrap();
        let ret_delta = runtime.block_on(work).unwrap().unwrap();
        assert_eq!(delta2, ret_delta);
    }
}

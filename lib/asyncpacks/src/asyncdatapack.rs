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

    use bytes::Bytes;
    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;
    use tempfile::TempDir;
    use tokio::runtime::Runtime;

    use revisionstore::{DataPackVersion, Delta, Metadata, MutableDataPack, MutablePack};
    use types::{Key, Node};

    fn make_datapack(
        tempdir: &TempDir,
        deltas: &Vec<(Delta, Metadata)>,
    ) -> impl Future<Item = AsyncDataPack, Error = Error> + 'static {
        let mut mutdatapack = MutableDataPack::new(tempdir.path(), DataPackVersion::One).unwrap();
        for (delta, metadata) in deltas.iter() {
            mutdatapack.add(delta, metadata).unwrap();
        }

        let path = mutdatapack.close().unwrap();

        AsyncDataPack::new(path)
    }

    #[test]
    fn test_one_delta() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new().unwrap();

        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(Key::new(vec![0], Node::random(&mut rng))),
            key: Key::new(vec![0], Node::random(&mut rng)),
        };
        let revisions = vec![(delta.clone(), Default::default())];

        let work = make_datapack(&tempdir, &revisions);
        let key = delta.key.clone();
        let work = work.and_then(move |datapack| datapack.get_delta(&key));

        let mut runtime = Runtime::new().unwrap();
        let ret_delta = runtime.block_on(work).unwrap();
        assert_eq!(delta, ret_delta);
    }

    #[test]
    fn test_multiple_delta() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new().unwrap();

        let delta1 = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(Key::new(vec![0], Node::random(&mut rng))),
            key: Key::new(vec![0], Node::random(&mut rng)),
        };
        let delta2 = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(Key::new(vec![0], Node::random(&mut rng))),
            key: Key::new(vec![0], Node::random(&mut rng)),
        };
        let revisions = vec![
            (delta1.clone(), Default::default()),
            (delta2.clone(), Default::default()),
        ];

        let work = make_datapack(&tempdir, &revisions);
        let key1 = delta1.key.clone();
        let key2 = delta2.key.clone();

        let work = work.and_then(move |datapack| {
            let delta = datapack.get_delta(&key1);
            delta.and_then(move |delta| {
                assert_eq!(delta, delta1);
                datapack.get_delta(&key2)
            })
        });

        let mut runtime = Runtime::new().unwrap();
        let ret_delta = runtime.block_on(work).unwrap();
        assert_eq!(delta2, ret_delta);
    }
}

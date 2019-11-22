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

use revisionstore::IndexedLogDataStore;

use crate::asyncmutabledeltastore::AsyncMutableDeltaStore;

pub type AsyncMutableIndexedLogDataStore = AsyncMutableDeltaStore<IndexedLogDataStore>;

impl AsyncMutableIndexedLogDataStore {
    pub fn new(dir: PathBuf) -> impl Future<Item = Self, Error = Error> + Send + 'static {
        poll_fn(move || blocking(|| IndexedLogDataStore::new(&dir)))
            .from_err()
            .and_then(move |res| res)
            .map(move |res| AsyncMutableDeltaStore::new_(res))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use bytes::Bytes;
    use tempfile::tempdir;
    use tokio::runtime::Runtime;

    use revisionstore::{DataStore, Delta};
    use types::{Key, RepoPathBuf};

    #[test]
    fn test_add() {
        let tempdir = tempdir().unwrap();

        let log = AsyncMutableIndexedLogDataStore::new(tempdir.path().to_path_buf());

        let delta = Delta {
            data: Bytes::from(&[0, 1, 2][..]),
            base: None,
            key: Key::new(RepoPathBuf::new(), Default::default()),
        };

        let cloned_delta = delta.clone();
        let work = log.and_then(move |log| log.add(&delta, &Default::default()));
        let work = work.and_then(move |log| log.close());

        let mut runtime = Runtime::new().unwrap();
        runtime.block_on(work).unwrap();

        let log = IndexedLogDataStore::new(tempdir.path()).unwrap();
        let stored_delta = log.get_delta(&cloned_delta.key).unwrap().unwrap();
        assert_eq!(stored_delta, cloned_delta);
    }
}

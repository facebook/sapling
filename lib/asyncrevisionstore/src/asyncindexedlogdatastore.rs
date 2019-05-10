// Copyright 2019 Facebook, Inc.

use std::path::PathBuf;

use failure::Error;
use futures::future::poll_fn;
use tokio::prelude::*;
use tokio_threadpool::blocking;

use cloned::cloned;
use revisionstore::{Delta, IndexedLogDataStore, Metadata, MutableDeltaStore};

pub struct AsyncMutableIndexedLogDataStore {
    inner: Option<IndexedLogDataStore>,
}

impl AsyncMutableIndexedLogDataStore {
    pub fn new(dir: PathBuf) -> impl Future<Item = Self, Error = Error> + Send + 'static {
        poll_fn(move || blocking(|| IndexedLogDataStore::new(&dir)))
            .from_err()
            .and_then(move |res| res)
            .map(move |res| AsyncMutableIndexedLogDataStore { inner: Some(res) })
    }

    pub fn add(
        mut self,
        delta: &Delta,
        metadata: &Metadata,
    ) -> impl Future<Item = Self, Error = Error> + Send + 'static {
        poll_fn({
            cloned!(delta, metadata);
            move || {
                blocking(|| {
                    let inner = self.inner.take();
                    let mut inner = inner.expect("The indexedlog is closed");
                    inner.add(&delta, &metadata).map(|()| inner)
                })
            }
        })
        .from_err()
        .and_then(|res| res)
        .map(move |inner| AsyncMutableIndexedLogDataStore { inner: Some(inner) })
    }

    pub fn close(mut self) -> impl Future<Item = (), Error = Error> + Send + 'static {
        poll_fn({
            move || {
                blocking(|| {
                    let inner = self.inner.take();
                    let inner = inner.expect("The indexedlog is closed");
                    inner.close().map(|_| ())
                })
            }
        })
        .from_err()
        .and_then(|res| res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use bytes::Bytes;
    use tempfile::tempdir;
    use tokio::runtime::Runtime;

    use revisionstore::DataStore;
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
        let stored_delta = log.get_delta(&cloned_delta.key).unwrap();
        assert_eq!(stored_delta, cloned_delta);
    }
}

// Copyright 2019 Facebook, Inc.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use failure::Error;
use futures::future::poll_fn;
use tokio::prelude::*;
use tokio_threadpool::blocking;

use cloned::cloned;
use revisionstore::{DataPackVersion, Delta, Metadata, MutableDataPack, MutablePack};

struct AsyncMutableDataPackInner {
    data: MutableDataPack,
}

/// Wraps a MutableDataPack to be used in an asynchronous context.
///
/// The API is designed to consume the `AsyncMutableDataPack` and return it, this allows chaining
/// the Futures with `and_then()`.
///
/// # Examples
/// ```
/// let mutablepack = AsyncMutableDataPack::new(path, DataPackVersion::One);
/// let work = mutablepack
///     .and_then(move |datapack| datapack.add(&delta1, None))
///     .and_then(move |datapack| datapack.add(&delta2, None))
///     .and_then(move |datapack| datapack.close()
/// ```
pub struct AsyncMutableDataPack {
    inner: Arc<Mutex<Option<AsyncMutableDataPackInner>>>,
}

impl AsyncMutableDataPack {
    /// Build an AsyncMutableDataPack.
    pub fn new(
        dir: PathBuf,
        version: DataPackVersion,
    ) -> impl Future<Item = Self, Error = Error> + Send + 'static {
        poll_fn(move || blocking(|| MutableDataPack::new(&dir, version.clone())))
            .from_err()
            .and_then(move |res| res)
            .map(move |res| AsyncMutableDataPack {
                inner: Arc::new(Mutex::new(Some(AsyncMutableDataPackInner { data: res }))),
            })
    }

    /// Add the `Delta` to this datapack.
    pub fn add(
        self,
        delta: &Delta,
        metadata: Option<Metadata>,
    ) -> impl Future<Item = Self, Error = Error> + Send + 'static {
        poll_fn({
            cloned!(delta, self.inner);
            move || {
                blocking(|| {
                    let mut inner = inner.lock().expect("Poisoned Mutex");
                    let inner = inner.as_mut();
                    let inner = inner.expect("The datapack is closed");
                    inner.data.add(&delta, metadata.clone())
                })
            }
        })
        .from_err()
        .and_then(|res| res)
        .map(move |()| AsyncMutableDataPack {
            inner: Arc::clone(&self.inner),
        })
    }

    /// Close the mutabledatapack. Once this Future finishes, the pack file is written to the disk
    /// and its path is returned.
    pub fn close(self) -> impl Future<Item = PathBuf, Error = Error> + Send + 'static {
        poll_fn({
            move || {
                blocking(|| {
                    let mut inner = self.inner.lock().expect("Poisoned Mutex");
                    let inner = inner.take();
                    let inner = inner.expect("The datapack is closed");
                    inner.data.close()
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

    use revisionstore::{DataPack, DataStore};
    use types::Key;


    #[test]
    fn test_add() {
        let tempdir = tempdir().unwrap();

        let mutabledatapack =
            AsyncMutableDataPack::new(tempdir.path().to_path_buf(), DataPackVersion::One);

        let delta = Delta {
            data: Bytes::from(&[0, 1, 2][..]),
            base: None,
            key: Key::new(Vec::new(), Default::default()),
        };

        let cloned_delta = delta.clone();
        let work = mutabledatapack.and_then(move |datapack| datapack.add(&delta, None));
        let work = work.and_then(move |datapack| datapack.close());

        let mut runtime = Runtime::new().unwrap();
        let datapackbase = runtime.block_on(work).unwrap();
        let path = datapackbase.with_extension("datapack");

        let pack = DataPack::new(&path).unwrap();
        let stored_delta = pack.get_delta(&cloned_delta.key).unwrap();
        assert_eq!(stored_delta, cloned_delta);
    }

    #[test]
    fn test_close() {
        let tempdir = tempdir().unwrap();

        let mutabledatapack =
            AsyncMutableDataPack::new(tempdir.path().to_path_buf(), DataPackVersion::One);
        let work = mutabledatapack.and_then(move |datapack| datapack.close());
        let mut runtime = Runtime::new().unwrap();

        let datapackbase = runtime.block_on(work).unwrap();
        let path = datapackbase.with_extension("datapack");
        assert!(path.exists());
    }
}

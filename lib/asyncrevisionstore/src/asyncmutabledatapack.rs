// Copyright 2019 Facebook, Inc.

use std::path::PathBuf;

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
///     .and_then(move |datapack| datapack.add(&delta1, &meta1))
///     .and_then(move |datapack| datapack.add(&delta2, &meta2))
///     .and_then(move |datapack| datapack.close()
/// ```
pub struct AsyncMutableDataPack {
    inner: Option<AsyncMutableDataPackInner>,
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
                inner: Some(AsyncMutableDataPackInner { data: res }),
            })
    }

    /// Add the `Delta` to this datapack.
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
                    let mut inner = inner.expect("The datapack is closed");
                    inner.data.add(&delta, &metadata).map(|()| inner)
                })
            }
        })
        .from_err()
        .and_then(|res| res)
        .map(move |inner| AsyncMutableDataPack { inner: Some(inner) })
    }

    /// Close the mutabledatapack. Once this Future finishes, the pack file is written to the disk
    /// and its path is returned.
    pub fn close(mut self) -> impl Future<Item = PathBuf, Error = Error> + Send + 'static {
        poll_fn({
            move || {
                blocking(|| {
                    let inner = self.inner.take();
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
        let datapackbase = runtime.block_on(work).unwrap();
        let path = datapackbase.with_extension("datapack");

        let pack = DataPack::new(&path).unwrap();
        let stored_delta = pack.get_delta(&cloned_delta.key).unwrap();
        assert_eq!(stored_delta, cloned_delta);
    }

    #[test]
    fn test_empty_close() {
        let tempdir = tempdir().unwrap();

        let mutabledatapack =
            AsyncMutableDataPack::new(tempdir.path().to_path_buf(), DataPackVersion::One);
        let work = mutabledatapack.and_then(move |datapack| datapack.close());
        let mut runtime = Runtime::new().unwrap();

        let datapackbase = runtime.block_on(work).unwrap();
        assert_eq!(datapackbase, PathBuf::new());
    }
}

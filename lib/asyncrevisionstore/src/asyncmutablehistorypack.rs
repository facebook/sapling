// Copyright 2019 Facebook, Inc.

use std::path::PathBuf;

use failure::Error;
use futures::future::poll_fn;
use tokio::prelude::*;
use tokio_threadpool::blocking;

use cloned::cloned;
use revisionstore::{HistoryPackVersion, MutableHistoryPack, MutableHistoryStore};
use types::{HistoryEntry, Key, NodeInfo};

pub struct AsyncMutableHistoryPackInner {
    data: MutableHistoryPack,
}

/// Wraps a MutableHistoryPack to be used in an asynchronous context.
///
/// The API is designed to consume the `AsyncMutableHistoryPack` and return it, this allows
/// chaining the Futures with `and_then()`.
///
/// # Examples
/// ```
/// let mutablepack = AsyncMutableHistoryPack::new(path, HistoryPackVersion::One);
/// let work = mutablepack
///     .and_then(move |historypack| historypack.add(&key1, &nodeinfo1))
///     .and_then(move |historypack| historypack.add(&key2, &nodeinfo2))
///     .and_then(move |historypack| historypack.close()
/// ```
pub struct AsyncMutableHistoryPack {
    inner: Option<AsyncMutableHistoryPackInner>,
}

impl AsyncMutableHistoryPack {
    /// Build an AsyncMutableHistoryPack.
    pub fn new(
        dir: PathBuf,
        version: HistoryPackVersion,
    ) -> impl Future<Item = Self, Error = Error> + Send + 'static {
        poll_fn(move || blocking(|| MutableHistoryPack::new(&dir, version.clone())))
            .from_err()
            .and_then(move |res| res)
            .map(move |res| AsyncMutableHistoryPack {
                inner: Some(AsyncMutableHistoryPackInner { data: res }),
            })
    }

    /// Add the `NodeInfo` to this historypack.
    pub fn add(
        mut self,
        key: &Key,
        info: &NodeInfo,
    ) -> impl Future<Item = Self, Error = Error> + Send + 'static {
        poll_fn({
            cloned!(key, info);
            move || {
                blocking(|| {
                    let inner = self.inner.take();
                    let mut inner = inner.expect("The historypack is closed");
                    inner.data.add(&key, &info).map(|()| inner)
                })
            }
        })
        .from_err()
        .and_then(|res| res)
        .map(move |inner| AsyncMutableHistoryPack { inner: Some(inner) })
    }

    /// Convenience function for adding a `types::PackHistoryEntry`.
    pub fn add_entry(
        self,
        entry: &HistoryEntry,
    ) -> impl Future<Item = Self, Error = Error> + Send + 'static {
        self.add(&entry.key, &entry.nodeinfo)
    }

    /// Close the historypack. Once this Future finishes, the pack file will be written to the disk
    /// and its path is returned.
    pub fn close(mut self) -> impl Future<Item = Option<PathBuf>, Error = Error> + Send + 'static {
        poll_fn({
            move || {
                blocking(|| {
                    let inner = self.inner.take();
                    let mut inner = inner.expect("The historypack is closed");
                    inner.data.flush()
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

    use tempfile::tempdir;
    use tokio::runtime::Runtime;

    use revisionstore::{HistoryPack, HistoryStore};
    use types::testutil::*;

    #[test]
    fn test_empty_close() {
        let tempdir = tempdir().unwrap();

        let mutablehistorypack =
            AsyncMutableHistoryPack::new(tempdir.path().to_path_buf(), HistoryPackVersion::One);
        let work = mutablehistorypack.and_then(move |historypack| historypack.close());
        let mut runtime = Runtime::new().unwrap();

        let historypackpath = runtime.block_on(work).unwrap().unwrap();
        assert_eq!(historypackpath, PathBuf::new());
    }

    #[test]
    fn test_add() {
        let tempdir = tempdir().unwrap();

        let file = "a/b";
        let my_key = key(&file, "2");
        let info = NodeInfo {
            parents: [key(&file, "1"), null_key(&file)],
            linknode: node("100"),
        };

        let keycloned = my_key.clone();
        let infocloned = info.clone();

        let mutablehistorypack =
            AsyncMutableHistoryPack::new(tempdir.path().to_path_buf(), HistoryPackVersion::One);
        let work = mutablehistorypack.and_then(move |historypack| {
            historypack
                .add(&keycloned, &infocloned)
                .and_then(move |historypack| historypack.close())
        });
        let mut runtime = Runtime::new().unwrap();

        let historypackpath = runtime.block_on(work).unwrap().unwrap();
        let path = historypackpath.with_extension("histpack");

        let pack = HistoryPack::new(&path).unwrap();

        assert_eq!(pack.get_node_info(&my_key).unwrap(), info);
    }
}

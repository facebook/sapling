// Copyright 2019 Facebook, Inc.

use std::path::PathBuf;

use failure::Error;
use futures::future::poll_fn;
use tokio::prelude::*;
use tokio_threadpool::blocking;

use revisionstore::{HistoryPackVersion, MutableHistoryPack};

use crate::asyncmutablehistorystore::AsyncMutableHistoryStore;

pub type AsyncMutableHistoryPack = AsyncMutableHistoryStore<MutableHistoryPack>;

impl AsyncMutableHistoryPack {
    /// Build an AsyncMutableHistoryPack.
    pub fn new(
        dir: PathBuf,
        version: HistoryPackVersion,
    ) -> impl Future<Item = Self, Error = Error> + Send + 'static {
        poll_fn(move || blocking(|| MutableHistoryPack::new(&dir, version.clone())))
            .from_err()
            .and_then(move |res| res)
            .map(move |historypack| AsyncMutableHistoryStore::new_(historypack))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;
    use tokio::runtime::Runtime;

    use revisionstore::{HistoryPack, HistoryStore};
    use types::{testutil::*, NodeInfo};

    #[test]
    fn test_empty_close() {
        let tempdir = tempdir().unwrap();

        let mutablehistorypack =
            AsyncMutableHistoryPack::new(tempdir.path().to_path_buf(), HistoryPackVersion::One);
        let work = mutablehistorypack.and_then(move |historypack| historypack.close());
        let mut runtime = Runtime::new().unwrap();

        let historypackpath = runtime.block_on(work).unwrap();
        assert_eq!(historypackpath, None);
    }

    #[test]
    fn test_add() {
        let tempdir = tempdir().unwrap();

        let file = "a/b";
        let my_key = key(&file, "2");
        let info = NodeInfo {
            parents: [key(&file, "1"), null_key(&file)],
            linknode: hgid("100"),
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

        assert_eq!(pack.get_node_info(&my_key).unwrap().unwrap(), info);
    }
}

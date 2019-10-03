// Copyright 2019 Facebook, Inc.

use std::path::PathBuf;

use failure::Error;
use futures::future::poll_fn;
use tokio::prelude::*;
use tokio_threadpool::blocking;

use revisionstore::IndexedLogHistoryStore;

use crate::asyncmutablehistorystore::AsyncMutableHistoryStore;

pub type AsyncMutableIndexedLogHistoryStore = AsyncMutableHistoryStore<IndexedLogHistoryStore>;

impl AsyncMutableIndexedLogHistoryStore {
    pub fn new(dir: PathBuf) -> impl Future<Item = Self, Error = Error> + Send + 'static {
        poll_fn(move || blocking(|| IndexedLogHistoryStore::new(&dir)))
            .from_err()
            .and_then(move |res| res)
            .map(move |res| AsyncMutableHistoryStore::new_(res))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;
    use tokio::runtime::Runtime;

    use revisionstore::HistoryStore;
    use types::{testutil::*, NodeInfo};

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

        let mutablehistorystore =
            AsyncMutableIndexedLogHistoryStore::new(tempdir.path().to_path_buf());
        let work = mutablehistorystore.and_then(move |historystore| {
            historystore
                .add(&keycloned, &infocloned)
                .and_then(move |historystore| historystore.close())
        });
        let mut runtime = Runtime::new().unwrap();

        let _ = runtime.block_on(work).unwrap();

        let store = IndexedLogHistoryStore::new(tempdir.path().to_path_buf()).unwrap();

        assert_eq!(store.get_node_info(&my_key).unwrap().unwrap(), info);
    }
}

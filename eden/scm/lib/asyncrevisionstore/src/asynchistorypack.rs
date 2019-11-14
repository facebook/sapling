/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use failure::Error;
use futures::future::poll_fn;
use tokio::prelude::*;
use tokio_threadpool::blocking;

use revisionstore::HistoryPack;

use crate::asynchistorystore::AsyncHistoryStore;

pub type AsyncHistoryPack = AsyncHistoryStore<HistoryPack>;

impl AsyncHistoryPack {
    pub fn new(
        path: PathBuf,
    ) -> impl Future<Item = AsyncHistoryPack, Error = Error> + Send + 'static {
        poll_fn({ move || blocking(|| HistoryPack::new(&path)) })
            .from_err()
            .and_then(|res| res)
            .map(move |historypack| AsyncHistoryStore::new_(historypack))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use tempfile::TempDir;
    use tokio::runtime::Runtime;

    use cloned::cloned;
    use futures_ext::FutureExt;
    use revisionstore::{HistoryPackVersion, MutableHistoryPack, MutableHistoryStore};
    use types::{testutil::*, Key, NodeInfo};

    fn make_historypack(
        tempdir: &TempDir,
        nodes: &HashMap<Key, NodeInfo>,
    ) -> impl Future<Item = AsyncHistoryPack, Error = Error> + 'static {
        let mutpack = MutableHistoryPack::new(tempdir.path(), HistoryPackVersion::One).unwrap();
        for (ref key, ref info) in nodes.iter() {
            mutpack.add(key.clone(), info.clone()).unwrap();
        }

        let path = mutpack.flush().unwrap().unwrap();
        AsyncHistoryPack::new(path)
    }

    // XXX: we should unify this and historypack.rs

    fn get_nodes() -> HashMap<Key, NodeInfo> {
        let mut nodes = HashMap::new();

        let file1 = "a";
        let file2 = "a/b";

        // Insert key 1
        let key1 = key(&file1, "2");
        let info = NodeInfo {
            parents: [key(&file1, "1"), null_key(&file1)],
            linknode: hgid("101"),
        };
        nodes.insert(key1.clone(), info.clone());

        // Insert key 2
        let key2 = key(&file2, "3");
        let info = NodeInfo {
            parents: [key(&file2, "5"), key(&file2, "6")],
            linknode: hgid("102"),
        };
        nodes.insert(key2.clone(), info.clone());

        // Insert key 3
        let key3 = key(&file1, "4");
        let info = NodeInfo {
            parents: [key2.clone(), key1.clone()],
            linknode: hgid("102"),
        };
        nodes.insert(key3.clone(), info.clone());

        nodes
    }

    #[test]
    fn test_get_node_info() {
        let tempdir = TempDir::new().unwrap();

        let nodes = get_nodes();

        let mut work = make_historypack(&tempdir, &nodes).boxify();
        for (key, info) in nodes.iter() {
            cloned!(key, info);
            work = work
                .and_then(move |historypack| {
                    historypack.get_node_info(&key).map(move |response| {
                        assert_eq!(response.unwrap(), info);
                        historypack
                    })
                })
                .boxify();
        }

        let mut runtime = Runtime::new().unwrap();
        runtime.block_on(work).expect("get_node_info failed");
    }
}

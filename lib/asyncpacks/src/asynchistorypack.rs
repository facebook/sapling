// Copyright 2019 Facebook, Inc.

use std::path::PathBuf;

use failure::Error;
use futures::future::poll_fn;
use tokio::prelude::*;
use tokio_threadpool::blocking;

use revisionstore::HistoryPack;

use crate::asynchistorystore::AsyncHistoryStore;

pub type AsyncHistoryPack = AsyncHistoryStore<HistoryPack>;
pub struct AsyncHistoryPackBuilder {}

impl AsyncHistoryPackBuilder {
    pub fn new(
        path: PathBuf,
    ) -> impl Future<Item = AsyncHistoryPack, Error = Error> + Send + 'static {
        poll_fn({ move || blocking(|| HistoryPack::new(&path)) })
            .from_err()
            .and_then(|res| res)
            .map(move |historypack| AsyncHistoryStore::new(historypack))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;
    use tempfile::TempDir;
    use tokio::runtime::Runtime;

    use cloned::cloned;
    use futures_ext::FutureExt;
    use revisionstore::{
        Ancestors, HistoryPackVersion, Key, MutableHistoryPack, MutablePack, NodeInfo,
    };
    use types::node::Node;

    fn make_historypack(
        tempdir: &TempDir,
        nodes: &HashMap<Key, NodeInfo>,
    ) -> impl Future<Item = AsyncHistoryPack, Error = Error> + 'static {
        let mut mutpack = MutableHistoryPack::new(tempdir.path(), HistoryPackVersion::One).unwrap();
        for (ref key, ref info) in nodes.iter() {
            mutpack.add(key.clone(), info.clone()).unwrap();
        }

        let path = mutpack.close().unwrap();
        AsyncHistoryPackBuilder::new(path)
    }

    // XXX: copy/pasted from historypack.rs
    fn get_nodes(mut rng: &mut ChaChaRng) -> (HashMap<Key, NodeInfo>, HashMap<Key, Ancestors>) {
        let file1 = vec![1, 2, 3];
        let file2 = vec![1, 2, 3, 4, 5];
        let null = Node::null_id();
        let node1 = Node::random(&mut rng);
        let node2 = Node::random(&mut rng);
        let node3 = Node::random(&mut rng);
        let node4 = Node::random(&mut rng);
        let node5 = Node::random(&mut rng);
        let node6 = Node::random(&mut rng);

        let mut nodes = HashMap::new();
        let mut ancestor_map = HashMap::new();

        // Insert key 1
        let key1 = Key::new(file1.clone(), node2.clone());
        let info = NodeInfo {
            parents: [
                Key::new(file1.clone(), node1.clone()),
                Key::new(file1.clone(), null.clone()),
            ],
            linknode: Node::random(&mut rng),
        };
        nodes.insert(key1.clone(), info.clone());
        let mut ancestors = HashMap::new();
        ancestors.insert(key1.clone(), info.clone());
        ancestor_map.insert(key1.clone(), ancestors);

        // Insert key 2
        let key2 = Key::new(file2.clone(), node3.clone());
        let info = NodeInfo {
            parents: [
                Key::new(file2.clone(), node5.clone()),
                Key::new(file2.clone(), node6.clone()),
            ],
            linknode: Node::random(&mut rng),
        };
        nodes.insert(key2.clone(), info.clone());
        let mut ancestors = HashMap::new();
        ancestors.insert(key2.clone(), info.clone());
        ancestor_map.insert(key2.clone(), ancestors);

        // Insert key 3
        let key3 = Key::new(file1.clone(), node4.clone());
        let info = NodeInfo {
            parents: [key2.clone(), key1.clone()],
            linknode: Node::random(&mut rng),
        };
        nodes.insert(key3.clone(), info.clone());
        let mut ancestors = HashMap::new();
        ancestors.insert(key3.clone(), info.clone());
        ancestors.extend(ancestor_map.get(&key2).unwrap().clone());
        ancestors.extend(ancestor_map.get(&key1).unwrap().clone());
        ancestor_map.insert(key3.clone(), ancestors);

        (nodes, ancestor_map)
    }

    #[test]
    fn test_get_ancestors() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new().unwrap();

        let (nodes, ancestors) = get_nodes(&mut rng);

        let mut work = make_historypack(&tempdir, &nodes).boxify();
        for (key, _) in nodes.iter() {
            cloned!(key, ancestors);
            work = work
                .and_then(move |historypack| {
                    historypack.get_ancestors(&key).map(move |response| {
                        assert_eq!(&response, ancestors.get(&key).unwrap());
                        historypack
                    })
                })
                .boxify();
        }

        let mut runtime = Runtime::new().unwrap();
        runtime.block_on(work).expect("get_ancestors failed");
    }

    #[test]
    fn test_get_node_info() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new().unwrap();

        let (nodes, _) = get_nodes(&mut rng);

        let mut work = make_historypack(&tempdir, &nodes).boxify();
        for (key, info) in nodes.iter() {
            cloned!(key, info);
            work = work
                .and_then(move |historypack| {
                    historypack.get_node_info(&key).map(move |response| {
                        assert_eq!(response, info);
                        historypack
                    })
                })
                .boxify();
        }

        let mut runtime = Runtime::new().unwrap();
        runtime.block_on(work).expect("get_node_info failed");
    }
}

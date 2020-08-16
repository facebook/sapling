/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use futures::prelude::*;

use types::{Key, NodeInfo};

use crate::{
    historystore::{HgIdHistoryStore, HgIdMutableHistoryStore, RemoteHistoryStore},
    localstore::LocalStore,
    types::StoreKey,
};

use super::{hgid_keys, EdenApiRemoteStore, File};

/// A history store backed by an `EdenApiRemoteStore` and a mutable store.
///
/// This type can only be created from an `EdenApiRemoteStore<File>`; attempting
/// to create one from a remote store for trees will panic since EdenAPI does
/// not support fetching tree history.
///
/// Data will be fetched over the network via the remote store and stored in the
/// mutable store before being returned to the caller. This type is not exported
/// because it is intended to be used as a trait object.
pub(super) struct EdenApiHistoryStore {
    remote: Arc<EdenApiRemoteStore<File>>,
    store: Arc<dyn HgIdMutableHistoryStore>,
}

impl EdenApiHistoryStore {
    pub(super) fn new(
        remote: Arc<EdenApiRemoteStore<File>>,
        store: Arc<dyn HgIdMutableHistoryStore>,
    ) -> Self {
        Self { remote, store }
    }
}

impl RemoteHistoryStore for EdenApiHistoryStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<()> {
        let client = self.remote.client.clone();
        let repo = self.remote.repo.clone();
        let keys = hgid_keys(keys);

        let fetch = async move {
            let mut response = client.history(repo, keys, None, None).await?;
            while let Some(entry) = response.entries.try_next().await? {
                self.store.add_entry(&entry)?;
            }
            Ok(())
        };

        let mut rt = self.remote.runtime.lock();
        rt.block_on(fetch)
    }
}

impl HgIdHistoryStore for EdenApiHistoryStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        self.prefetch(&[StoreKey::hgid(key.clone())])?;
        self.store.get_node_info(key)
    }
}

impl LocalStore for EdenApiHistoryStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.store.get_missing(keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use maplit::hashmap;
    use tempfile::TempDir;

    use configparser::config::ConfigSet;
    use types::testutil::*;

    use crate::{
        edenapi::{File, Tree},
        indexedloghistorystore::IndexedLogHgIdHistoryStore,
        remotestore::HgIdRemoteStore,
        testutil::*,
    };

    #[test]
    fn test_file_history() -> Result<()> {
        // Set up mocked EdenAPI store.
        let k = key("a", "1");
        let n = NodeInfo {
            parents: [key("b", "2"), null_key("a")],
            linknode: hgid("3"),
        };
        let history = hashmap! { k.clone() => n.clone() };

        let client = FakeEdenApi::new().history(history).into_arc();
        let remote = EdenApiRemoteStore::<File>::new("repo", client.clone())?;

        // Set up local mutable store to write received data.
        let tmp = TempDir::new()?;
        let local = Arc::new(IndexedLogHgIdHistoryStore::new(&tmp, &ConfigSet::new())?);

        // Set up `EdenApiHistoryStore`.
        let edenapi = remote.historystore(local.clone());

        // Attempt fetch.
        let nodeinfo = edenapi.get_node_info(&k)?.expect("history not found");
        assert_eq!(&nodeinfo, &n);

        // Check that data was written to the local store.
        let nodeinfo = local.get_node_info(&k)?.expect("history not found");
        assert_eq!(&nodeinfo, &n);

        Ok(())
    }

    #[test]
    #[should_panic]
    fn test_tree_history() {
        let client = FakeEdenApi::new().into_arc();
        let remote = EdenApiRemoteStore::<Tree>::new("repo", client.clone()).unwrap();

        // Set up local mutable store to write received data.
        let tmp = TempDir::new().unwrap();
        let local = Arc::new(IndexedLogHgIdHistoryStore::new(&tmp, &ConfigSet::new()).unwrap());

        // EdenAPI does not support fetching tree history, so it should
        // not be possible to get a history store from a tree store.
        // The following line should panic.
        let _ = remote.historystore(local);
    }
}

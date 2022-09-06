/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use async_runtime::block_on;
use futures::prelude::*;
use progress_model::ProgressBar;
use types::Key;
use types::NodeInfo;

use super::hgid_keys;
use super::EdenApiRemoteStore;
use super::File;
use crate::historystore::HgIdHistoryStore;
use crate::historystore::HgIdMutableHistoryStore;
use crate::historystore::RemoteHistoryStore;
use crate::localstore::LocalStore;
use crate::types::StoreKey;

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
        let keys = hgid_keys(keys);

        let response = async move {
            let prog =
                ProgressBar::register_new("Downloading file history over HTTP", 0, "entries");

            let mut response = client.history(keys, None).await?;
            while let Some(entry) = response.entries.try_next().await? {
                self.store.add_entry(&entry)?;
                prog.increase_position(1);
            }

            Ok(())
        };

        block_on(response)
    }
}

impl HgIdHistoryStore for EdenApiHistoryStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        self.prefetch(&[StoreKey::hgid(key.clone())])?;
        self.store.get_node_info(key)
    }

    fn refresh(&self) -> Result<()> {
        Ok(())
    }
}

impl LocalStore for EdenApiHistoryStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use maplit::hashmap;
    use tempfile::TempDir;
    use types::testutil::*;

    use super::*;
    use crate::edenapi::File;
    use crate::edenapi::Tree;
    use crate::indexedloghistorystore::IndexedLogHgIdHistoryStore;
    use crate::indexedlogutil::StoreType;
    use crate::remotestore::HgIdRemoteStore;
    use crate::testutil::*;

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
        let remote = EdenApiRemoteStore::<File>::new(client);

        // Set up local mutable store to write received data.
        let tmp = TempDir::new()?;
        let local = Arc::new(IndexedLogHgIdHistoryStore::new(
            &tmp,
            &empty_config(),
            StoreType::Shared,
        )?);

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
        let remote = EdenApiRemoteStore::<Tree>::new(client);

        // Set up local mutable store to write received data.
        let tmp = TempDir::new().unwrap();
        let local = Arc::new(
            IndexedLogHgIdHistoryStore::new(&tmp, &empty_config(), StoreType::Shared).unwrap(),
        );

        // EdenAPI does not support fetching tree history, so it should
        // not be possible to get a history store from a tree store.
        // The following line should panic.
        let _ = remote.historystore(local);
    }
}

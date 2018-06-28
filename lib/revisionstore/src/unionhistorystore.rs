// Copyright Facebook, Inc. 2018
// Union history store
use std::collections::VecDeque;
use std::rc::Rc;

use error::{KeyError, Result};
use historystore::{Ancestors, HistoryStore, NodeInfo};
use key::Key;
use unionstore::UnionStore;

pub type UnionHistoryStore = UnionStore<Rc<HistoryStore>>;

#[derive(Debug, Fail)]
#[fail(display = "Union History Store Error: {:?}", _0)]
struct UnionHistoryStoreError(String);

impl From<UnionHistoryStoreError> for KeyError {
    fn from(err: UnionHistoryStoreError) -> Self {
        KeyError::new(err.into())
    }
}

impl UnionHistoryStore {
    fn get_partial_ancestors(&self, key: &Key) -> Result<Ancestors> {
        for store in self {
            match store.get_ancestors(key) {
                Ok(res) => return Ok(res),
                Err(e) => match e.downcast_ref::<KeyError>() {
                    Some(_) => continue,
                    None => return Err(e),
                },
            }
        }

        Err(KeyError::from(UnionHistoryStoreError(format!(
            "No ancestors found for key {:?}",
            key
        ))).into())
    }
}

impl HistoryStore for UnionHistoryStore {
    fn get_ancestors(&self, key: &Key) -> Result<Ancestors> {
        let mut missing_ancestors = VecDeque::new();
        let mut ancestors = Ancestors::new();

        missing_ancestors.push_back(key.clone());
        while let Some(current) = missing_ancestors.pop_front() {
            if ancestors.contains_key(&current) {
                continue;
            }

            let partial_ancestors = self.get_partial_ancestors(&current)?;

            for ancestor in partial_ancestors.values() {
                for parent in &ancestor.parents {
                    if !partial_ancestors.contains_key(parent) {
                        missing_ancestors.push_back(parent.clone());
                    }
                }
            }

            ancestors.extend(partial_ancestors);
        }

        Ok(ancestors)
    }

    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        let initial_keys = Ok(keys.iter().cloned().collect());
        self.into_iter()
            .fold(initial_keys, |missing_keys, store| match missing_keys {
                Ok(missing_keys) => store.get_missing(&missing_keys),
                Err(e) => Err(e),
            })
    }

    fn get_node_info(&self, key: &Key) -> Result<NodeInfo> {
        for store in self {
            match store.get_node_info(key) {
                Ok(res) => return Ok(res),
                Err(e) => match e.downcast_ref::<KeyError>() {
                    Some(_) => continue,
                    None => return Err(e),
                },
            }
        }

        Err(KeyError::from(UnionHistoryStoreError(format!(
            "No NodeInfo found for key {:?}",
            key
        ))).into())
    }
}

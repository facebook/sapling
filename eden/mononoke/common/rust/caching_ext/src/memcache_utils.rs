/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::mock_store::MockStore;
use anyhow::Result;
use bytes::Bytes;
use memcache::MemcacheClient;
use memcache::MemcacheSetType;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[derive(Clone)]
pub enum MemcacheHandler {
    Real(MemcacheClient),
    #[allow(dead_code)]
    Mock(MockStore<Bytes>),
}

impl From<MemcacheClient> for MemcacheHandler {
    fn from(client: MemcacheClient) -> Self {
        MemcacheHandler::Real(client)
    }
}

impl MemcacheHandler {
    pub async fn get(&self, key: String) -> Result<Option<Bytes>> {
        match self {
            MemcacheHandler::Real(ref client) => {
                client.get(key).await.map(|value| value.map(Bytes::from))
            }
            MemcacheHandler::Mock(store) => Ok(store.get(&key)),
        }
    }

    pub async fn set<V>(&self, key: String, value: V) -> Result<()>
    where
        MemcacheSetType: From<V>,
        Bytes: From<V>,
        V: 'static,
    {
        match self {
            MemcacheHandler::Real(ref client) => client.set(key, value).await,
            MemcacheHandler::Mock(store) => {
                store.set(&key, value.into());
                Ok(())
            }
        }
    }

    pub async fn set_with_ttl<V>(&self, key: String, value: V, duration: Duration) -> Result<()>
    where
        MemcacheSetType: From<V>,
        Bytes: From<V>,
        V: 'static,
    {
        match self {
            MemcacheHandler::Real(ref client) => client.set_with_ttl(key, value, duration).await,
            MemcacheHandler::Mock(_) => {
                // For now we ignore TTLs here
                self.set(key, value).await
            }
        }
    }

    #[allow(dead_code)]
    pub fn create_mock() -> Self {
        MemcacheHandler::Mock(MockStore::new())
    }

    #[allow(dead_code)]
    pub(crate) fn gets_count(&self) -> usize {
        match self {
            MemcacheHandler::Real(_) => unimplemented!(),
            MemcacheHandler::Mock(MockStore { ref get_count, .. }) => {
                get_count.load(Ordering::SeqCst)
            }
        }
    }
}

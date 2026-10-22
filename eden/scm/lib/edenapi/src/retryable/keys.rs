/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::hash_map::RandomState;
use std::hash::Hash;

use async_trait::async_trait;
use futures::future::BoxFuture;
use hashbrown::Equivalent;
use hashbrown::HashSet;
use types::HgId;
use types::Key;
use types::RepoPathBuf;

use super::RetryableStreamRequest;
use crate::client::Client;
use crate::errors::SaplingRemoteApiError;
use crate::response::Response;

pub(crate) struct RetryableKeys<K, F, G> {
    pending: HashSet<K, RandomState>,
    fetch: F,
    received: G,
}

impl<K: Eq + Hash, F, G> RetryableKeys<K, F, G> {
    pub(crate) fn new<T>(keys: impl IntoIterator<Item = K>, fetch: F, received: G) -> Self
    where
        G: Fn(&mut HashSet<K, RandomState>, &T),
    {
        Self {
            pending: keys.into_iter().collect(),
            fetch,
            received,
        }
    }
}

#[async_trait]
impl<K, F, G, T> RetryableStreamRequest for RetryableKeys<K, F, G>
where
    K: Clone + Eq + Hash + Send + Sync + 'static,
    F: Fn(Client, Vec<K>) -> BoxFuture<'static, Result<Response<T>, SaplingRemoteApiError>>
        + Send
        + Sync
        + 'static,
    G: Fn(&mut HashSet<K, RandomState>, &T) + Send + Sync + 'static,
    T: Send + 'static,
{
    type Item = T;

    async fn perform(&self, client: Client) -> Result<Response<T>, SaplingRemoteApiError> {
        if self.pending.is_empty() {
            return Ok(Response::empty());
        }
        (self.fetch)(client, self.pending.iter().cloned().collect()).await
    }

    fn received_item(&mut self, item: &T) {
        (self.received)(&mut self.pending, item);
    }
}

#[derive(Hash)]
pub(crate) struct BorrowedKey<'a> {
    pub(crate) path: &'a RepoPathBuf,
    pub(crate) hgid: HgId,
}

impl Equivalent<Key> for BorrowedKey<'_> {
    fn equivalent(&self, key: &Key) -> bool {
        self.path == &key.path && self.hgid == key.hgid
    }
}

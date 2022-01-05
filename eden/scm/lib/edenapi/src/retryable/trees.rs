/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use async_trait::async_trait;
use types::Key;

use super::RetryableStreamRequest;
use crate::client::Client;
use crate::errors::EdenApiError;
use crate::response::Response;
use crate::types::EdenApiServerError;
use crate::types::TreeAttributes;
use crate::types::TreeEntry;

pub(crate) struct RetryableTrees {
    keys: HashSet<Key>,
    attributes: Option<TreeAttributes>,
}

impl RetryableTrees {
    pub(crate) fn new(
        keys: impl IntoIterator<Item = Key>,
        attributes: Option<TreeAttributes>,
    ) -> Self {
        let keys = keys.into_iter().collect();
        Self { keys, attributes }
    }
}

#[async_trait]
impl RetryableStreamRequest for RetryableTrees {
    type Item = Result<TreeEntry, EdenApiServerError>;

    async fn perform(&self, client: Client) -> Result<Response<Self::Item>, EdenApiError> {
        let keys: Vec<Key> = self.keys.iter().cloned().collect();
        client.fetch_trees(keys, self.attributes.clone()).await
    }

    fn received_item(&mut self, item: &Self::Item) {
        let key = match item {
            Ok(entry) => Some(entry.key()),
            Err(e) => e.key.as_ref(),
        };
        if let Some(key) = key {
            self.keys.remove(key);
        }
    }
}

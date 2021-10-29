/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use types::Key;

use crate::client::Client;
use crate::errors::EdenApiError;
use crate::response::Response;
use crate::types::{FileEntry, FileSpec};

use super::RetryableStreamRequest;

pub(crate) struct RetryableFiles {
    keys: HashSet<Key>,
}

impl RetryableFiles {
    pub(crate) fn new(keys: impl IntoIterator<Item = Key>) -> Self {
        let keys = keys.into_iter().collect();
        Self { keys }
    }
}

#[async_trait]
impl RetryableStreamRequest for RetryableFiles {
    type Item = FileEntry;

    async fn perform(
        &self,
        client: Client,
        repo: String,
    ) -> Result<Response<Self::Item>, EdenApiError> {
        let keys = self.keys.iter().cloned().collect();
        client.fetch_files(repo, keys).await
    }

    fn received_item(&mut self, item: &Self::Item) {
        self.keys.remove(item.key());
    }
}

pub(crate) struct RetryableFileAttrs {
    reqs: HashMap<Key, FileSpec>,
}

impl RetryableFileAttrs {
    pub(crate) fn new(reqs: impl IntoIterator<Item = FileSpec>) -> Self {
        let reqs = reqs.into_iter().map(|req| (req.key.clone(), req)).collect();
        Self { reqs }
    }
}

#[async_trait]
impl RetryableStreamRequest for RetryableFileAttrs {
    type Item = FileEntry;

    async fn perform(
        &self,
        client: Client,
        repo: String,
    ) -> Result<Response<Self::Item>, EdenApiError> {
        let reqs = self.reqs.values().cloned().collect();
        client.fetch_files_attrs(repo, reqs).await
    }

    fn received_item(&mut self, item: &Self::Item) {
        self.reqs.remove(item.key());
    }
}

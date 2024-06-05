/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use async_trait::async_trait;
use types::Key;

use super::RetryableStreamRequest;
use crate::client::Client;
use crate::errors::SaplingRemoteApiError;
use crate::response::Response;
use crate::types::FileResponse;
use crate::types::FileSpec;

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
    type Item = FileResponse;

    async fn perform(&self, client: Client) -> Result<Response<Self::Item>, SaplingRemoteApiError> {
        let reqs = self.reqs.values().cloned().collect();
        client.fetch_files_attrs(reqs).await
    }

    fn received_item(&mut self, item: &Self::Item) {
        self.reqs.remove(&item.key);
    }
}

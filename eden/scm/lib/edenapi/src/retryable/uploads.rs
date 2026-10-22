/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;

use async_trait::async_trait;
use edenapi_types::AnyId;
use edenapi_types::UploadTokensResponse;
use futures::future::BoxFuture;

use super::RetryableStreamRequest;
use crate::client::Client;
use crate::errors::SaplingRemoteApiError;
use crate::response::Response;

pub(crate) struct RetryableUploads<T, F> {
    pending: BTreeMap<usize, (T, Option<usize>)>,
    positions: HashMap<AnyId, usize>,
    fetch: F,
}

impl<T, F> RetryableUploads<T, F> {
    pub(crate) fn new(items: Vec<T>, key: impl Fn(&T) -> AnyId, fetch: F) -> Self {
        let mut positions = HashMap::with_capacity(items.len());
        let pending = items
            .into_iter()
            .enumerate()
            .map(|(position, item)| {
                // One acknowledgement completes all inputs with the same ID.
                let previous = positions.insert(key(&item), position);
                (position, (item, previous))
            })
            .collect();
        Self {
            pending,
            positions,
            fetch,
        }
    }
}

#[async_trait]
impl<T, F> RetryableStreamRequest for RetryableUploads<T, F>
where
    T: Clone + Send + Sync + 'static,
    F: Fn(
            Client,
            Vec<T>,
        )
            -> BoxFuture<'static, Result<Response<UploadTokensResponse>, SaplingRemoteApiError>>
        + Send
        + Sync
        + 'static,
{
    type Item = UploadTokensResponse;

    async fn perform(&self, client: Client) -> Result<Response<Self::Item>, SaplingRemoteApiError> {
        if self.pending.is_empty() {
            return Ok(Response::empty());
        }
        let items = self
            .pending
            .values()
            .map(|(item, _)| item.clone())
            .collect();
        (self.fetch)(client, items).await
    }

    fn received_item(&mut self, item: &Self::Item) {
        let mut position = self.positions.remove(&item.token.data.id);
        while let Some(index) = position {
            position = self
                .pending
                .remove(&index)
                .and_then(|(_, previous)| previous);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use edenapi_types::UploadToken;
    use futures::prelude::*;
    use parking_lot::Mutex;
    use types::HgId;

    use super::*;
    use crate::builder::HttpClientBuilder;

    #[tokio::test]
    async fn completed_uploads_are_dropped_and_retries_preserve_order() {
        let ids: Vec<_> = [3, 1, 3, 2].map(|n| HgId::from_byte_array([n; 20])).into();
        let items: Vec<_> = ids.iter().map(|id| (*id, Arc::new(()))).collect();
        let lifetimes: Vec<_> = items.iter().map(|(_, data)| Arc::downgrade(data)).collect();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let mut uploads = RetryableUploads::new(
            items,
            |(id, _)| AnyId::HgChangesetId(*id),
            move |_client, items: Vec<(HgId, Arc<()>)>| {
                let ids: Vec<_> = items.iter().map(|(id, _)| *id).collect();
                recorded.lock().push(ids.clone());
                async move {
                    let entries = stream::iter(ids.into_iter().map(|id| {
                        Ok(UploadTokensResponse {
                            token: UploadToken::new_fake_token(AnyId::HgChangesetId(id), None),
                        })
                    }))
                    .boxed();
                    Ok(Response {
                        entries,
                        ..Response::empty()
                    })
                }
                .boxed()
            },
        );
        let client = HttpClientBuilder::new()
            .repo_name("test")
            .server_url("http://127.0.0.1:1".parse().unwrap())
            .build()
            .unwrap();
        let responses = uploads
            .perform(client.clone())
            .await
            .unwrap()
            .flatten()
            .await
            .unwrap();
        uploads.received_item(&responses[1]);
        assert!(lifetimes[1].upgrade().is_none());
        assert!(lifetimes[0].upgrade().is_some());
        uploads.perform(client.clone()).await.unwrap();

        uploads.received_item(&responses[0]);
        assert!(lifetimes[0].upgrade().is_none());
        assert!(lifetimes[2].upgrade().is_none());
        assert!(lifetimes[3].upgrade().is_some());
        uploads.perform(client.clone()).await.unwrap();

        uploads.received_item(&responses[2]);
        uploads.received_item(&responses[3]);
        assert!(lifetimes.iter().all(|item| item.upgrade().is_none()));
        assert!(
            uploads
                .perform(client)
                .await
                .unwrap()
                .flatten()
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            *requests.lock(),
            vec![ids.clone(), vec![ids[0], ids[2], ids[3]], vec![ids[3]]]
        );
    }
}

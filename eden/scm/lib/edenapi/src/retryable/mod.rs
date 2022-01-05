/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use async_trait::async_trait;
use edenapi_trait::Entries;
use edenapi_trait::Response;
use futures::channel::mpsc;
use futures::prelude::*;
use http_client::Stats;

use crate::client::Client;
use crate::errors::EdenApiError;

mod files;
mod trees;

pub(crate) use files::RetryableFileAttrs;
pub(crate) use files::RetryableFiles;
pub(crate) use trees::RetryableTrees;

const MAX_RETRIES: usize = 3;
#[async_trait]
pub(crate) trait RetryableStreamRequest: Sized + Sync + Send + 'static {
    type Item: Send + 'static;

    async fn perform(&self, client: Client) -> Result<Response<Self::Item>, EdenApiError>;

    fn received_item(&mut self, _item: &Self::Item) {}

    fn retry_after(&mut self, error: &EdenApiError, attempt: usize) -> Option<Duration> {
        if error.is_retryable() && attempt < MAX_RETRIES {
            Some(Duration::from_secs(attempt as u64 + 1))
        } else {
            None
        }
    }

    async fn perform_with_retries(
        self,
        client: Client,
    ) -> Result<Response<Self::Item>, EdenApiError> {
        struct RetryState<R, T> {
            request: R,
            entries: Option<Entries<T>>,
            attempt: usize,
        }

        let state = RetryState {
            request: self,
            entries: None,
            attempt: 0,
        };

        let (stats_tx, stats_rx) = mpsc::channel(0);
        let entries = stream::unfold(state, move |mut state| {
            let mut stats_tx = stats_tx.clone();
            let client = client.clone();

            async move {
                loop {
                    // Ideally we'd return None when we hit the final error, but we need to use
                    // that time to return the error, so instead we return None on this next
                    // iteration.
                    if state.attempt > MAX_RETRIES {
                        return None;
                    }

                    let res = if let Some(ref mut entries) = state.entries {
                        tracing::trace!("Polling response stream");
                        if let Some(res) = entries.next().await {
                            tracing::trace!("Item received");
                            res
                        } else {
                            tracing::trace!("Transfer complete");
                            return None;
                        }
                    } else {
                        tracing::trace!("No active response stream; sending new request");
                        let res = state.request.perform(client.clone()).await;
                        match res {
                            Ok(Response { entries, stats, .. }) => {
                                state.entries = Some(entries);
                                let _ = stats_tx.try_send(stats);
                                continue;
                            }
                            Err(e) => Err(e),
                        }
                    };

                    let error = match res {
                        Ok(item) => {
                            state.request.received_item(&item);
                            return Some((Ok(item), state));
                        }
                        Err(e) => e,
                    };

                    let retry_after = match state.request.retry_after(&error, state.attempt) {
                        Some(d) => d,
                        None => {
                            state.attempt = MAX_RETRIES + 1;
                            return Some((Err(error), state));
                        }
                    };
                    state.attempt += 1;
                    state.entries = None;

                    tracing::error!(
                        "Retrying after {:#?} due to error (attempt {}): {}",
                        &retry_after,
                        state.attempt,
                        &error
                    );

                    tokio::time::sleep(retry_after).await;
                }
            }
        })
        .boxed();

        let stats = stats_rx.into_future().then(|(next, _tail)| match next {
            Some(fut) => fut,
            None => Box::pin(future::ok(Stats::default())),
        });

        Ok(Response {
            entries,
            stats: Box::pin(stats),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use anyhow::anyhow;
    use anyhow::Result;
    use async_runtime::block_on;
    use async_runtime::stream_to_iter;
    use async_trait::async_trait;
    use http_client::HttpClientError;
    use parking_lot::Mutex;
    use types::HgId;
    use types::Key;
    use types::RepoPathBuf;

    use super::*;
    use crate::builder::HttpClientBuilder;
    use crate::EdenApiError;
    use crate::Response;

    pub(crate) struct RetryableTest {
        pub keys: HashSet<Key>,
        fails: u32,
        pub attempts: u32,
    }

    impl RetryableTest {
        pub(crate) fn new(keys: Vec<Key>, fails: u32) -> Self {
            Self {
                keys: HashSet::from_iter(keys.into_iter()),
                fails,
                attempts: 0,
            }
        }
    }

    #[async_trait]
    impl RetryableStreamRequest for Arc<Mutex<RetryableTest>> {
        type Item = Key;

        async fn perform(&self, _client: Client) -> Result<Response<Self::Item>, EdenApiError> {
            let mut this = self.lock();
            let mut response = Response::empty();
            let mut entries: Vec<Result<Key, EdenApiError>> =
                this.keys.iter().map(|k| Ok(k.clone())).collect();

            this.attempts += 1;
            if this.attempts <= this.fails {
                entries[1] = Err(EdenApiError::Http(HttpClientError::BadResponse(anyhow!(
                    "fake error"
                ))));
            }
            response.entries = Box::pin(Box::new(stream::iter(entries.into_iter())));
            Ok(response)
        }

        fn received_item(&mut self, item: &Self::Item) {
            self.lock().keys.remove(item);
        }
    }

    #[test]
    fn test_retryable_trait() -> Result<()> {
        let base_url = "https://example.com".parse()?;
        let repo_name = "test_repo";

        let client = HttpClientBuilder::new()
            .repo_name(repo_name)
            .server_url(base_url)
            .build()?;

        let keys: Vec<Key> = vec![
            Key::new(
                RepoPathBuf::from_string("1".to_string()).unwrap(),
                HgId::null_id().clone(),
            ),
            Key::new(
                RepoPathBuf::from_string("2".to_string()).unwrap(),
                HgId::null_id().clone(),
            ),
            Key::new(
                RepoPathBuf::from_string("3".to_string()).unwrap(),
                HgId::null_id().clone(),
            ),
        ];
        let fails = 2;
        let retryable = Arc::new(Mutex::new(RetryableTest::new(keys, fails)));
        let retryable_move = retryable.clone();
        let response = block_on(retryable_move.perform_with_retries(client))?;

        let results: Vec<_> = stream_to_iter(response.entries).into_iter().collect();

        assert_eq!(retryable.lock().attempts, fails + 1);
        assert_eq!(retryable.lock().keys.len(), 0);
        let results: HashSet<String> =
            HashSet::from_iter(results.into_iter().map(|k| k.unwrap().path.into_string()));
        assert!(results.contains(&"1".to_string()));
        assert!(results.contains(&"2".to_string()));
        assert!(results.contains(&"3".to_string()));

        Ok(())
    }
}

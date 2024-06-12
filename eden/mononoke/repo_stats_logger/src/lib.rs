/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::Error;
use fbinit::FacebookInit;
use futures::future::abortable;
use futures::future::AbortHandle;
use stats::define_stats;
use stats::prelude::DynamicSingletonCounter;

define_stats! {
    prefix = "mononoke.app.repo.stats";
    repo_objects_count: dynamic_singleton_counter("{}.objects.count", (repo_name: String)),
}

const DEFAULT_REPO_OBJECTS_COUNT: i64 = 1_000_000;

#[derive(Clone)]
#[facet::facet]
pub struct RepoStatsLogger {
    abort_handle: AbortHandle,
}

impl RepoStatsLogger {
    pub async fn new(fb: FacebookInit, repo_name: String) -> Result<Self, Error> {
        let fut = async move {
            loop {
                let jitter = Duration::from_secs(60);
                tokio::time::sleep(jitter).await;

                STATS::repo_objects_count.set_value(
                    fb,
                    DEFAULT_REPO_OBJECTS_COUNT,
                    (repo_name.clone(),),
                );
            }
        };

        let (fut, abort_handle) = abortable(fut);
        tokio::spawn(fut);

        Ok(Self { abort_handle })
    }

    // A null implementation that does nothing. Useful for tests.
    pub fn noop() -> Self {
        Self {
            abort_handle: AbortHandle::new_pair().0,
        }
    }
}

impl Drop for RepoStatsLogger {
    fn drop(&mut self) {
        self.abort_handle.abort()
    }
}

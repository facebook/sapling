/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::Error;
use context::CoreContext;
use fbinit::expect_init;
use futures::future::abortable;
use futures::future::AbortHandle;
use slog::info;

#[derive(Clone)]
#[facet::facet]
pub struct RepoStatsLogger {
    abort_handle: AbortHandle,
}

impl RepoStatsLogger {
    pub async fn new(repo_name: String) -> Result<Self, Error> {
        // This code is called without a request so it can't take a CoreContext; we roll up our own.
        let fb = expect_init();
        let logger = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let ctx = CoreContext::new_for_bulk_processing(fb, logger);
        let name = repo_name.clone();

        let fut = async move {
            loop {
                let jitter = Duration::from_secs(60);
                tokio::time::sleep(jitter).await;

                info!(ctx.logger(), "RepoStatsLogger for {}", name);
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

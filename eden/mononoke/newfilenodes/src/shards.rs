/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use core::future::Future;
use futures_stats::TimedTryFutureExt;
use mercurial_types::HgFileNodeId;
use mononoke_types::RepoPath;
use stats::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;
use time_ext::DurationExt;
use tokio::sync::Semaphore;

define_stats! {
    prefix = "mononoke.filenodes";
    filenodes_shard_checkout_ms: histogram(10, 0, 1_000, Average, Count; P 5; P 25; P 50; P 75; P 95; P 99; P 100),
    history_shard_checkout_ms: histogram(10, 0, 1_000, Average, Count; P 5; P 25; P 50; P 75; P 95; P 99; P 100),
}

#[derive(Debug)]
pub struct Shards {
    filenodes: Vec<Semaphore>,
    history: Vec<Semaphore>,
}

impl Shards {
    pub fn new(filenodes_concurrency: usize, history_concurrency: usize) -> Self {
        let filenodes = (0..filenodes_concurrency)
            .into_iter()
            .map(|_| Semaphore::new(1))
            .collect();

        let history = (0..history_concurrency)
            .into_iter()
            .map(|_| Semaphore::new(1))
            .collect();

        Self { filenodes, history }
    }
}
impl Shards {
    pub fn with_filenodes<F, T, Fut>(
        self: Arc<Self>,
        path: &RepoPath,
        filenode_id: HgFileNodeId,
        f: F,
    ) -> tokio::task::JoinHandle<Result<T, Error>>
    where
        F: FnOnce() -> Fut + Send + 'static,
        T: Send + Sync + 'static,
        Fut: Future<Output = Result<T, Error>> + Send,
    {
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        filenode_id.hash(&mut hasher);
        let index = (hasher.finish() % self.filenodes.len() as u64) as usize;
        // We must task::spawn() the code that runs while the semaphore is acquired
        // in order to reduce the risk of deadlocks. See T102183795 for details.
        tokio::spawn(async move {
            let (stats, _permit) = self.filenodes[index].acquire().try_timed().await?;
            STATS::filenodes_shard_checkout_ms
                .add_value(stats.completion_time.as_millis_unchecked() as i64);
            f().await
        })
    }

    pub fn with_history<F, T, Fut>(
        self: Arc<Self>,
        path: &RepoPath,
        f: F,
    ) -> tokio::task::JoinHandle<Result<T, Error>>
    where
        F: FnOnce() -> Fut + Send + 'static,
        T: Send + Sync + 'static,
        Fut: Future<Output = Result<T, Error>> + Send,
    {
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        let index = (hasher.finish() % self.history.len() as u64) as usize;
        // We must task::spawn() the code that runs while the semaphore is acquired
        // in order to reduce the risk of deadlocks. See T102183795 for details.
        tokio::spawn(async move {
            let (stats, _permit) = self.history[index].acquire().try_timed().await?;
            STATS::history_shard_checkout_ms
                .add_value(stats.completion_time.as_millis_unchecked() as i64);
            f().await
        })
    }
}

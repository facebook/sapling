/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use futures_stats::TimedFutureExt;
use mercurial_types::HgFileNodeId;
use mononoke_types::RepoPath;
use stats::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use time_ext::DurationExt;
use tokio::sync::{Semaphore, SemaphorePermit};

define_stats! {
    prefix = "mononoke.filenodes";
    filenodes_shard_checkout_ms: histogram(10, 0, 1_000, Average, Count; P 5; P 25; P 50; P 75; P 95; P 99; P 100),
    history_shard_checkout_ms: histogram(10, 0, 1_000, Average, Count; P 5; P 25; P 50; P 75; P 95; P 99; P 100),
}

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
    pub async fn acquire_filenodes<'a>(
        &'a self,
        path: &RepoPath,
        filenode_id: HgFileNodeId,
    ) -> SemaphorePermit<'a> {
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        filenode_id.hash(&mut hasher);

        let (stats, permit) = self.filenodes
            [(hasher.finish() % self.filenodes.len() as u64) as usize]
            .acquire()
            .timed()
            .await;
        STATS::filenodes_shard_checkout_ms
            .add_value(stats.completion_time.as_millis_unchecked() as i64);

        permit
    }

    pub async fn acquire_history<'a>(&'a self, path: &RepoPath) -> SemaphorePermit<'a> {
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);

        let (stats, permit) = self.history[(hasher.finish() % self.history.len() as u64) as usize]
            .acquire()
            .timed()
            .await;
        STATS::history_shard_checkout_ms
            .add_value(stats.completion_time.as_millis_unchecked() as i64);

        permit
    }
}

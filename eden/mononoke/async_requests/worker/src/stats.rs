/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::time::Duration;

use async_requests::AsyncMethodRequestQueue;
use async_requests::types::RequestStatus;
use context::CoreContext;
use mononoke_api::RepositoryId;
use mononoke_types::Timestamp;
use requests_table::QueueStatsEntry;
use slog::warn;
use stats::define_stats;
use stats::prelude::*;

const STATS_LOOP_INTERNAL: Duration = Duration::from_secs(5 * 60);

const STATUSES: [RequestStatus; 4] = [
    RequestStatus::New,
    RequestStatus::InProgress,
    RequestStatus::Ready,
    RequestStatus::Polled,
];

define_stats! {
    prefix = "async_requests.worker.stats";

    stats_error: timeseries("error"; Count),
    queue_length_by_status: dynamic_singleton_counter("queue.length.{}", (status: String)),
    queue_age_by_status: dynamic_singleton_counter("queue.age_s.{}", (status: String)),
    queue_length_by_repo_and_status: dynamic_singleton_counter("queue.{}.length.{}", (repo_id: String, status: String)),
    queue_age_by_repo_and_status: dynamic_singleton_counter("queue.{}.age_s.{}", (repo_id: String, status: String)),
}

pub(crate) async fn stats_loop(
    ctx: &CoreContext,
    repo_ids: Vec<RepositoryId>,
    queue: &AsyncMethodRequestQueue,
) {
    loop {
        let res = queue.get_queue_stats(ctx).await;
        let now = Timestamp::now();
        match res {
            Ok(res) => {
                process_queue_length_by_status(ctx, &res);
                process_queue_age_by_status(ctx, now, &res);
                process_queue_length_by_repo_and_status(ctx, &repo_ids, &res);
                process_queue_age_by_repo_and_status(ctx, &repo_ids, now, &res);
            }
            Err(err) => {
                STATS::stats_error.add_value(1);
                warn!(
                    ctx.logger(),
                    "error while getting queue stats, skipping: {:?}", err
                );
            }
        }

        tokio::time::sleep(STATS_LOOP_INTERNAL).await;
    }
}

// Keep track of the stats we have already logged. Any missing ones, we will log a 0.
struct Seen {
    inner: HashMap<QueueStatsEntry, bool>,
}

impl Seen {
    fn new(repo_ids: &Vec<RepositoryId>) -> Self {
        let mut seen = HashMap::new();
        for status in STATUSES {
            for repo_id in repo_ids {
                seen.insert(
                    QueueStatsEntry {
                        repo_id: Some(repo_id.clone()),
                        status,
                    },
                    false,
                );
            }
        }
        Self { inner: seen }
    }

    fn mark(&mut self, repo_id: Option<RepositoryId>, status: RequestStatus) {
        self.inner.insert(QueueStatsEntry { repo_id, status }, true);
    }

    fn get_missing(&self) -> Vec<QueueStatsEntry> {
        self.inner
            .iter()
            .filter_map(|(entry, seen)| if !*seen { Some(entry) } else { None })
            .cloned()
            .collect()
    }
}

fn process_queue_length_by_status(ctx: &CoreContext, res: &requests_table::QueueStats) {
    let mut seen = Seen::new(&vec![]);
    let stats = &res.queue_length_by_status;
    for (status, count) in stats {
        seen.mark(None, *status);
        STATS::queue_length_by_status.set_value(ctx.fb, *count as i64, (status.to_string(),));
    }

    for entry in seen.get_missing() {
        STATS::queue_length_by_status.set_value(ctx.fb, 0, (entry.status.to_string(),));
    }
}

fn process_queue_age_by_status(
    ctx: &CoreContext,
    now: Timestamp,
    res: &requests_table::QueueStats,
) {
    let mut seen = Seen::new(&vec![]);
    let stats = &res.queue_age_by_status;
    for (status, ts) in stats {
        seen.mark(None, *status);
        let diff = std::cmp::max(now.timestamp_seconds() - ts.timestamp_seconds(), 0);
        STATS::queue_age_by_status.set_value(ctx.fb, diff, (status.to_string(),));
    }

    for entry in seen.get_missing() {
        STATS::queue_age_by_status.set_value(ctx.fb, 0, (entry.status.to_string(),));
    }
}

fn process_queue_length_by_repo_and_status(
    ctx: &CoreContext,
    repo_ids: &Vec<RepositoryId>,
    res: &requests_table::QueueStats,
) {
    let mut seen = Seen::new(repo_ids);
    let stats = &res.queue_length_by_repo_and_status;
    for (entry, count) in stats {
        seen.mark(entry.repo_id, entry.status);
        STATS::queue_length_by_repo_and_status.set_value(
            ctx.fb,
            *count as i64,
            (
                entry.repo_id.unwrap_or(RepositoryId::new(0)).to_string(),
                entry.status.to_string(),
            ),
        );
    }

    for entry in seen.get_missing() {
        STATS::queue_length_by_repo_and_status.set_value(
            ctx.fb,
            0,
            (
                entry.repo_id.unwrap_or(RepositoryId::new(0)).to_string(),
                entry.status.to_string(),
            ),
        );
    }
}

fn process_queue_age_by_repo_and_status(
    ctx: &CoreContext,
    repo_ids: &Vec<RepositoryId>,
    now: Timestamp,
    res: &requests_table::QueueStats,
) {
    let mut seen = Seen::new(repo_ids);
    let stats = &res.queue_age_by_repo_and_status;
    for (entry, ts) in stats {
        seen.mark(entry.repo_id, entry.status);
        let diff = std::cmp::max(now.timestamp_seconds() - ts.timestamp_seconds(), 0);
        STATS::queue_age_by_repo_and_status.set_value(
            ctx.fb,
            diff,
            (
                entry.repo_id.unwrap_or(RepositoryId::new(0)).to_string(),
                entry.status.to_string(),
            ),
        );
    }

    for entry in seen.get_missing() {
        STATS::queue_age_by_repo_and_status.set_value(
            ctx.fb,
            0,
            (
                entry.repo_id.unwrap_or(RepositoryId::new(0)).to_string(),
                entry.status.to_string(),
            ),
        );
    }
}

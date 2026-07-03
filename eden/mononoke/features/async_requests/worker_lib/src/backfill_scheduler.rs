/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Sliding-window scheduler for multi-repo derived-data backfill.
//!
//! By default a `DeriveBackfill` root fans out a `DeriveBackfillRepo` request
//! for every repo at once, so a large repo list floods the derivation backends.
//! When `repo_concurrency > 0` the root instead acts as a long-lived scheduler:
//! it keeps at most `repo_concurrency` repos deriving at a time and schedules a
//! new one as each repo's derivation fully finishes.
//!
//! State is reconstructed from the queue on every poll (never held only in
//! memory), so the scheduler is safe across worker restarts: if the worker
//! running the root dies, the request is reclaimed as abandoned and resumed by
//! another worker, which recomputes what is scheduled / in flight / remaining
//! from the committed rows and continues topping up the window. Already-running
//! repos keep deriving independently the whole time.

use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;

use async_requests::AsyncMethodRequestQueue;
use async_requests::AsyncRequestsError;
use async_requests::types::RowId;
use context::CoreContext;
use mononoke_types::RepositoryId;
use requests_table::ChildCounts;
use source_control as thrift;
use tracing::info;
use tracing::warn;

use crate::backfill::enqueue_repo_backfill;

/// How often the scheduler polls the queue to notice completed repos and top up
/// the in-flight window. Must stay well below the worker's abandoned-request
/// threshold (5 minutes) so the long-lived scheduler request is never falsely
/// reclaimed while it is healthy.
const SCHEDULER_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Decision produced by [`plan_next_batch`].
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct BatchPlan {
    /// repo_ids to enqueue now (respects free slots and excludes scheduled).
    pub to_schedule: Vec<i64>,
    /// True once nothing remains to schedule and nothing is still in flight.
    pub done: bool,
}

/// Pure planning step for the sliding window. Given the full ordered repo list,
/// the per-repo child counts observed in the queue, the set of repos already
/// scheduled, and the concurrency limit, decide which repos to schedule next.
///
/// A repo counts as "in flight" when it is scheduled and either still has
/// pending (new/inprogress) child requests, or is not yet visible in the counts
/// (e.g. just enqueued and not yet replicated to the read replica) — treating an
/// unseen just-scheduled repo as in flight prevents transiently exceeding the
/// window. `repo_concurrency` bounds in_flight plus newly scheduled repos.
pub(crate) fn plan_next_batch(
    full_repo_ids: &[i64],
    per_repo_counts: &HashMap<RepositoryId, ChildCounts>,
    already_scheduled: &HashSet<i64>,
    repo_concurrency: usize,
) -> BatchPlan {
    let in_flight = already_scheduled
        .iter()
        .filter(|repo_id| {
            per_repo_counts
                .get(&RepositoryId::new(**repo_id as i32))
                .is_none_or(ChildCounts::is_pending)
        })
        .count();

    let remaining: Vec<i64> = full_repo_ids
        .iter()
        .copied()
        .filter(|repo_id| !already_scheduled.contains(repo_id))
        .collect();

    if remaining.is_empty() && in_flight == 0 {
        return BatchPlan {
            to_schedule: Vec::new(),
            done: true,
        };
    }

    let free = repo_concurrency.saturating_sub(in_flight);
    let to_schedule = remaining.into_iter().take(free).collect();

    BatchPlan {
        to_schedule,
        done: false,
    }
}

/// Drive a multi-repo backfill with a bounded rolling window of `repo_concurrency`
/// repos. Runs until every repo has been fully derived. Transient queue errors
/// are retried on the next poll rather than surfaced as `Err`, since returning
/// `Err` would cascade-fail the whole backfill subtree.
pub(crate) async fn schedule_repo_backfills(
    ctx: &CoreContext,
    queue: &AsyncMethodRequestQueue,
    params: &thrift::DeriveBackfillParams,
    root_request_id: &RowId,
    created_by: Option<&str>,
    repo_concurrency: usize,
) -> Result<(), AsyncRequestsError> {
    let full_repo_ids: Vec<i64> = params
        .repo_entries
        .iter()
        .map(|entry| entry.repo_id)
        .collect();
    let mut already_scheduled: HashSet<i64> = HashSet::new();

    loop {
        let counts = match queue
            .get_backfill_child_counts_by_repo(ctx, root_request_id)
            .await
        {
            Ok(counts) => counts,
            Err(err) => {
                warn!(
                    "backfill scheduler: failed to fetch stats for root {}, retrying: {err:#}",
                    root_request_id.0,
                );
                tokio::time::sleep(SCHEDULER_POLL_INTERVAL).await;
                continue;
            }
        };

        // Anything already visible in the queue was scheduled by a previous
        // iteration (or by an earlier scheduler instance before a restart).
        for repo_id in counts.keys() {
            already_scheduled.insert(repo_id.id() as i64);
        }

        info!(
            "backfill scheduler: scheduling next batch, already scheduled: {already_scheduled:?}"
        );
        let plan = plan_next_batch(
            &full_repo_ids,
            &counts,
            &already_scheduled,
            repo_concurrency,
        );
        if plan.done {
            break;
        }

        // Enqueue the next batch of repos.
        info!(
            "backfill scheduler: scheduling {} repos",
            plan.to_schedule.len()
        );
        for repo_id in &plan.to_schedule {
            let Some(entry) = params
                .repo_entries
                .iter()
                .find(|entry| entry.repo_id == *repo_id)
            else {
                continue;
            };
            match enqueue_repo_backfill(ctx, queue, params, entry, root_request_id, created_by)
                .await
            {
                Ok(()) => {
                    already_scheduled.insert(*repo_id);
                }
                // Leave it unscheduled so the next poll retries it.
                Err(err) => warn!(
                    "backfill scheduler: failed to enqueue repo {repo_id}, will retry: {err:#}",
                ),
            }
        }

        tokio::time::sleep(SCHEDULER_POLL_INTERVAL).await;
    }

    info!(
        "backfill scheduler: all {} repos fully derived (root {})",
        full_repo_ids.len(),
        root_request_id.0,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    fn pending() -> ChildCounts {
        ChildCounts {
            inprogress: 1,
            ..Default::default()
        }
    }

    fn done() -> ChildCounts {
        ChildCounts {
            ready: 3,
            ..Default::default()
        }
    }

    fn repo(id: i64) -> RepositoryId {
        RepositoryId::new(id as i32)
    }

    #[mononoke::test]
    fn schedules_first_window_when_nothing_started() {
        let plan = plan_next_batch(&[1, 2, 3, 4], &HashMap::new(), &HashSet::new(), 2);
        assert_eq!(plan.to_schedule, vec![1, 2], "should fill the empty window");
        assert!(!plan.done);
    }

    #[mononoke::test]
    fn window_full_schedules_nothing() {
        let counts = HashMap::from([(repo(1), pending()), (repo(2), pending())]);
        let scheduled = HashSet::from([1, 2]);
        let plan = plan_next_batch(&[1, 2, 3, 4], &counts, &scheduled, 2);
        assert!(
            plan.to_schedule.is_empty(),
            "no free slots while 2 in flight"
        );
        assert!(!plan.done);
    }

    #[mononoke::test]
    fn refills_exactly_the_freed_slots() {
        // repo 1 finished, repo 2 still deriving -> one slot free -> schedule repo 3 only.
        let counts = HashMap::from([(repo(1), done()), (repo(2), pending())]);
        let scheduled = HashSet::from([1, 2]);
        let plan = plan_next_batch(&[1, 2, 3, 4], &counts, &scheduled, 2);
        assert_eq!(plan.to_schedule, vec![3]);
        assert!(!plan.done);
    }

    #[mononoke::test]
    fn just_scheduled_but_not_yet_visible_counts_as_in_flight() {
        // repo 1 was scheduled this process but hasn't appeared in the counts yet
        // (replica lag). It must count as in flight so we don't exceed the window.
        let counts = HashMap::new();
        let scheduled = HashSet::from([1]);
        let plan = plan_next_batch(&[1, 2, 3], &counts, &scheduled, 2);
        assert_eq!(plan.to_schedule, vec![2], "only one extra slot is free");
        assert!(!plan.done);
    }

    #[mononoke::test]
    fn done_only_when_all_scheduled_and_none_pending() {
        let counts = HashMap::from([
            (repo(1), done()),
            (repo(2), done()),
            (repo(3), done()),
            (repo(4), done()),
        ]);
        let scheduled = HashSet::from([1, 2, 3, 4]);
        let plan = plan_next_batch(&[1, 2, 3, 4], &counts, &scheduled, 2);
        assert!(plan.to_schedule.is_empty());
        assert!(plan.done, "all repos derived -> finished");
    }

    #[mononoke::test]
    fn not_done_while_last_repo_still_pending() {
        let counts = HashMap::from([
            (repo(1), done()),
            (repo(2), done()),
            (repo(3), done()),
            (repo(4), pending()),
        ]);
        let scheduled = HashSet::from([1, 2, 3, 4]);
        let plan = plan_next_batch(&[1, 2, 3, 4], &counts, &scheduled, 2);
        assert!(plan.to_schedule.is_empty(), "nothing left to schedule");
        assert!(!plan.done, "one repo still deriving");
    }
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Aggregation of raw backfill request statuses into user-facing progress.
//!
//! A backfill is a DAG of requests sharing a `root_request_id`. The raw
//! `RequestStatus::Ready` on a parent only means that request finished
//! *spawning* its children — the children (the actual derivation work) may
//! still be running. These helpers aggregate across child statuses so that
//! both the `backfill-status` admin command and the worker-side scheduler can
//! answer "is this repo still deriving?" with the same rule.

use std::collections::HashMap;

use crate::RequestStatus;

/// Counts of child requests grouped by their effective state. All four
/// counts together describe a backfill's progress without exposing the raw
/// `RequestStatus` enum to callers that just want to render a status or decide
/// whether more work is outstanding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChildCounts {
    pub new: u64,
    pub inprogress: u64,
    pub ready: u64,
    pub failed: u64,
}

impl ChildCounts {
    pub fn from_status_map(statuses: &HashMap<RequestStatus, usize>) -> Self {
        let get = |s| *statuses.get(&s).unwrap_or(&0) as u64;
        ChildCounts {
            new: get(RequestStatus::New),
            inprogress: get(RequestStatus::InProgress),
            ready: get(RequestStatus::Ready) + get(RequestStatus::Polled),
            failed: get(RequestStatus::Failed),
        }
    }

    pub fn total(&self) -> u64 {
        self.new + self.inprogress + self.ready + self.failed
    }

    /// Whether any child work is still outstanding (queued or running). This is
    /// the signal the scheduler uses to decide a repo is still "in flight" —
    /// a repo is only done once nothing is `new` or `inprogress`.
    pub fn is_pending(&self) -> bool {
        self.new > 0 || self.inprogress > 0
    }
}

/// User-facing status for a backfill or repo, derived from raw request statuses.
///
/// The raw `RequestStatus::Ready` only means a request finished spawning
/// children; the children may still be running. Aggregating across child
/// statuses gives a status that matches what the user actually cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepoStatus {
    NotStarted,
    InProgress,
    Completed,
    Failed,
}

impl std::fmt::Display for RepoStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            RepoStatus::NotStarted => "new",
            RepoStatus::InProgress => "inprogress",
            RepoStatus::Completed => "completed",
            RepoStatus::Failed => "failed",
        };
        write!(f, "{s}")
    }
}

impl RepoStatus {
    /// Determine the overall status of a repo based on its child request counts.
    ///
    /// Used for per-repo aggregation where the "root" status is just the
    /// children's status — there is no separate root request to factor in.
    pub fn from_child_counts(counts: ChildCounts) -> Self {
        if counts.failed > 0 {
            RepoStatus::Failed
        } else if counts.inprogress > 0 || (counts.ready > 0 && counts.new > 0) {
            RepoStatus::InProgress
        } else if counts.ready > 0 {
            RepoStatus::Completed
        } else {
            RepoStatus::NotStarted
        }
    }

    /// Determine the overall status of a backfill from the root request's
    /// own status plus child counts. The root reaches `Ready` once it
    /// finishes spawning child requests, even though the children may still
    /// be running — so for a user-facing status we need to look across both.
    pub fn from_root_and_children(root_status: RequestStatus, children: ChildCounts) -> Self {
        if root_status == RequestStatus::Failed {
            return RepoStatus::Failed;
        }
        let from_children = Self::from_child_counts(children);
        match (from_children, root_status) {
            // No children spawned yet but root is still spawning them.
            (RepoStatus::NotStarted, RequestStatus::InProgress | RequestStatus::New) => {
                RepoStatus::InProgress
            }
            (other, _) => other,
        }
    }
}

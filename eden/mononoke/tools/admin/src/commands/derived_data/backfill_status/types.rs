/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(dead_code)]

use std::collections::HashMap;
use std::time::Duration;

use mononoke_types::ChangesetId;
use mononoke_types::Timestamp;
use requests_table::LongRunningRequestEntry;
use requests_table::RequestStatus;
use requests_table::RowId;

/// Overall summary of a backfill job
pub(super) struct BackfillSummary {
    pub root_request_id: RowId,
    pub created_at: Timestamp,
    pub derived_data_type: String,
    pub total_repos: usize,
    pub status_counts: HashMap<RequestStatus, usize>,
    pub type_counts: HashMap<String, usize>,
    pub repos_by_status: HashMap<RepoStatus, Vec<i64>>,
    pub timing_stats: TimingStats,
}

/// Timing and performance metrics for a backfill
pub(super) struct TimingStats {
    pub elapsed_time: Duration,
    pub completed_count: usize,
    pub avg_duration: Option<Duration>,
    pub requests_per_hour: f64,
    pub estimated_remaining: Option<Duration>,
}

/// User-facing status for a backfill or repo, derived from raw request statuses.
///
/// The raw `RequestStatus::Ready` only means a request finished spawning
/// children; the children may still be running. Aggregating across child
/// statuses gives a status that matches what the user actually cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum RepoStatus {
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
        write!(f, "{}", s)
    }
}

/// Progress details for a specific repository
pub(super) struct RepoProgress {
    pub repo_id: i64,
    pub repo_name: Option<String>,
    pub status: RepoStatus,
    pub request_counts: HashMap<(String, RequestStatus), usize>,
}

impl TimingStats {
    /// Calculate estimated time remaining based on completion rate
    pub fn calculate_estimated_remaining(&self, pending_count: usize) -> Option<Duration> {
        if self.requests_per_hour > 0.0 && self.elapsed_time.as_secs() > 300 {
            let hours_remaining = pending_count as f64 / self.requests_per_hour;
            Some(Duration::from_secs_f64(hours_remaining * 3600.0))
        } else {
            None
        }
    }
}

/// Common display data for backfill status views
pub(super) struct BackfillDisplayData {
    pub request_id: RowId,
    pub created_at: Timestamp,
    pub created_by: Option<String>,
    pub aggregate_status: RepoStatus,
    pub request_type: String,
    pub derived_data_type: Option<String>,
    pub total_requests: usize,
    pub status_counts: Vec<(RequestStatus, usize)>,
    pub type_breakdown: Vec<(String, Vec<(RequestStatus, usize)>)>,
    pub elapsed_time: Duration,
    pub avg_duration: Option<Duration>,
    pub requests_per_hour: f64,
    pub estimated_remaining: Option<Duration>,
}

/// Display data for repo-specific drill-down view
pub(super) struct RepoDisplayData {
    pub request_id: RowId,
    pub repo_id: i64,
    pub repo_name: Option<String>,
    pub overall_status: RepoStatus,
    pub derived_data_type: Option<String>,
    pub total_requests: usize,
    pub status_counts: Vec<(RequestStatus, usize)>,
    pub type_breakdown: Vec<(String, Vec<(RequestStatus, usize)>)>,
}

/// Decoded parameters for an individual backfill child request.
pub(super) enum BackfillChildParams {
    DeriveBoundaries {
        repo_id: i64,
        derived_data_type: String,
        boundary_cs_ids: Vec<ChangesetId>,
        concurrency: i32,
        use_predecessor_derivation: bool,
        config_name: Option<String>,
    },
    DeriveSlice {
        repo_id: i64,
        derived_data_type: String,
        segments: Vec<SliceSegmentDisplayData>,
        config_name: Option<String>,
    },
}

pub(super) struct SliceSegmentDisplayData {
    pub head: String,
    pub base: String,
}

/// Decoded result for an individual backfill child request, when available.
pub(super) enum BackfillChildResult {
    DeriveBoundaries {
        derived_count: i64,
        error_message: Option<String>,
    },
    DeriveSlice {
        derived_count: i64,
        error_message: Option<String>,
    },
    Error {
        message: String,
    },
}

/// Derived-data mapping status for boundary changesets in a derive_boundaries request.
pub(super) enum BoundaryDerivationStatus {
    Checked {
        already_derived_count: usize,
        not_derived_count: usize,
    },
    NotChecked {
        reason: String,
    },
}

pub(super) struct BackfillChildDisplayData {
    pub entry: LongRunningRequestEntry,
    pub params: BackfillChildParams,
    pub result: Option<BackfillChildResult>,
    pub boundary_derivation_status: Option<BoundaryDerivationStatus>,
}

pub(super) struct RepoDetailRow {
    pub repo_id: i64,
    pub repo_name: Option<String>,
    pub status: RepoStatus,
    pub derived: usize,
    pub total: usize,
}

/// Counts of child requests grouped by their effective state. All four
/// counts together describe a backfill's progress without exposing the raw
/// `RequestStatus` enum to callers that just want to render a status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ChildCounts {
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

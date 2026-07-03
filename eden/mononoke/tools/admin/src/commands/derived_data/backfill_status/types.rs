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

/// Backfill progress aggregation is shared with the worker-side scheduler, so
/// `ChildCounts` and `RepoStatus` live in `requests_table` (both crates apply
/// the same "is this repo still deriving?" rule). Re-exported here so existing
/// `self::types::{RepoStatus, ChildCounts}` references keep resolving.
pub(super) use requests_table::ChildCounts;
pub(super) use requests_table::RepoStatus;

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

/// Settings the backfill was enqueued with, decoded from the root request's
/// `DeriveBackfillParams` blob. These are the knobs passed to
/// `backfill-enqueue` (slice size, concurrency, etc.).
pub(super) struct BackfillSettings {
    pub slice_size: i64,
    pub boundaries_concurrency: i32,
    pub num_boundary_requests: i32,
    pub reslice: bool,
    pub config_name: Option<String>,
}

/// Common display data for backfill status views
pub(super) struct BackfillDisplayData {
    pub request_id: RowId,
    pub created_at: Timestamp,
    pub created_by: Option<String>,
    pub aggregate_status: RepoStatus,
    pub request_type: String,
    pub derived_data_type: Option<String>,
    pub settings: Option<BackfillSettings>,
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

/// A single child request (derive_boundaries / derive_slice) of a backfill,
/// rendered as a row in the detailed child-request view. `repo_id` is only
/// rendered for multi-repo backfills, where it disambiguates which repository
/// each child request belongs to.
pub(super) struct ChildRequestRow {
    pub id: u64,
    pub repo_id: Option<i64>,
    pub request_type: String,
    pub status: RequestStatus,
    pub claimed_by: Option<String>,
}

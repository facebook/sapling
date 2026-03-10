/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(dead_code)]

use std::collections::HashMap;
use std::time::Duration;

use mononoke_types::Timestamp;
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

/// Status of a repository in a backfill
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum RepoStatus {
    NotStarted,
    InProgress,
    Completed,
    Failed,
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
    pub status: RequestStatus,
    pub request_type: String,
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
    pub overall_status: String,
    pub total_requests: usize,
    pub status_counts: Vec<(RequestStatus, usize)>,
    pub type_breakdown: Vec<(String, Vec<(RequestStatus, usize)>)>,
}

impl RepoStatus {
    /// Determine the overall status of a repo based on its request statuses
    pub fn from_request_statuses(statuses: &HashMap<RequestStatus, usize>) -> Self {
        let has_failed = statuses.contains_key(&RequestStatus::Failed);
        let has_inprogress = statuses.contains_key(&RequestStatus::InProgress);
        let ready_count = statuses.get(&RequestStatus::Ready).unwrap_or(&0);
        let polled_count = statuses.get(&RequestStatus::Polled).unwrap_or(&0);
        let completed = ready_count + polled_count;
        let total: usize = statuses.values().sum();

        if has_failed {
            RepoStatus::Failed
        } else if completed == total && total > 0 {
            RepoStatus::Completed
        } else if has_inprogress {
            RepoStatus::InProgress
        } else {
            RepoStatus::NotStarted
        }
    }
}

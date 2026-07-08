/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Unified per-repo "bookmark saturation" counters, shared by the pushrebase
//! land path and the non-pushrebase bookmark-move path so both report under one
//! ODS key (`mononoke.bookmark_saturation`). All durations are nanoseconds.
//!
//! These cover every bookmark move that contends on the per-repo
//! `bookmarks_update_log` serialization. Emission is gated by callers to repos
//! that configure `monitoring_bookmark`, matching the legacy per-repo metric.

use stats::prelude::*;

define_stats! {
    prefix = "mononoke.bookmark_saturation";
    critical_section_success_duration_ns: dynamic_timeseries("{}.critical_section_success_duration_ns", (reponame: String); Average, Sum, Count),
    critical_section_failure_duration_ns: dynamic_timeseries("{}.critical_section_failure_duration_ns", (reponame: String); Average, Sum, Count),
    critical_section_retries_failed: dynamic_timeseries("{}.critical_section_retries_failed", (reponame: String); Average, Sum),
    commits_rebased: dynamic_timeseries("{}.commits_rebased", (reponame: String); Average, Sum, Count),
    bookmark_move_success_duration_ns: dynamic_timeseries("{}.bookmark_move_success_duration_ns", (reponame: String); Average, Sum, Count),
    bookmark_move_failure_duration_ns: dynamic_timeseries("{}.bookmark_move_failure_duration_ns", (reponame: String); Average, Sum, Count),
}

/// Record a successful pushrebase land: the critical-section duration, the
/// number of failed optimistic retries (only the classic retry loop tracks
/// this; batched paths pass `None`), and the number of rebased commits.
pub fn record_pushrebase_success(
    reponame: &str,
    duration_ns: i64,
    retries_failed: Option<i64>,
    commits_rebased: i64,
) {
    STATS::critical_section_success_duration_ns.add_value(duration_ns, (reponame.to_string(),));
    if let Some(retries) = retries_failed {
        STATS::critical_section_retries_failed.add_value(retries, (reponame.to_string(),));
    }
    STATS::commits_rebased.add_value(commits_rebased, (reponame.to_string(),));
}

/// Record a pushrebase attempt that lost the CAS race, with the critical-section
/// duration of the losing attempt.
pub fn record_pushrebase_failure(reponame: &str, duration_ns: i64) {
    STATS::critical_section_failure_duration_ns.add_value(duration_ns, (reponame.to_string(),));
}

/// Record the number of failed optimistic retries for a pushrebase land —
/// either the retries a land took before succeeding, or the full budget when it
/// gave up. Callers on the batched paths (which retry per-request via re-queue)
/// use this to report retries that the per-batch success/failure samples can't
/// carry.
pub fn record_pushrebase_retries(reponame: &str, retries_failed: i64) {
    STATS::critical_section_retries_failed.add_value(retries_failed, (reponame.to_string(),));
}

/// Record a non-pushrebase bookmark move (create/update/delete), timed at the
/// serialized commit that contends on the per-repo log ordering.
pub fn record_bookmark_move(reponame: &str, duration_ns: i64, success: bool) {
    if success {
        STATS::bookmark_move_success_duration_ns.add_value(duration_ns, (reponame.to_string(),));
    } else {
        STATS::bookmark_move_failure_duration_ns.add_value(duration_ns, (reponame.to_string(),));
    }
}

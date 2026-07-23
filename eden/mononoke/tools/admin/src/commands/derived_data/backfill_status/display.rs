/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

use chrono::DateTime;
use chrono::Utc;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use num_format::Buffer;
use num_format::CustomFormat;
use num_format::Grouping;
use prettytable::Cell;
use prettytable::Row;
use prettytable::Table;
use prettytable::format;
use requests_table::RequestStatus;
use requests_table::RowId;

use super::types::BackfillChildDisplayData;
use super::types::BackfillChildParams;
use super::types::BackfillChildResult;
use super::types::BackfillDisplayData;
use super::types::BackfillSettings;
use super::types::BoundaryDerivationStatus;
use super::types::ChildRequestRow;
use super::types::RepoDetailRow;
use super::types::RepoDisplayData;
use super::types::RepoStatus;

/// A row in the recent-backfills list view.
pub(super) struct BackfillListRow {
    pub request_id: RowId,
    pub created_at: Timestamp,
    pub created_by: Option<String>,
    pub aggregate_status: RepoStatus,
    pub has_failed_requests: bool,
    pub repo_count: i64,
    /// Distinct repo ids for this backfill, used to render repo names in the
    /// "Repos" column. May be empty if they couldn't be loaded.
    pub repo_ids: Vec<i64>,
    pub derived_data_type: Option<String>,
}

/// Translate a raw `RequestStatus` into a user-facing label for display.
/// `ready` and `polled` are both rendered as "completed" since the user-facing
/// notion of "this request finished successfully" is the same.
fn status_label(status: RequestStatus) -> &'static str {
    match status {
        RequestStatus::New => "new",
        RequestStatus::InProgress => "inprogress",
        RequestStatus::Ready | RequestStatus::Polled => "completed",
        RequestStatus::Failed => "failed",
    }
}

/// Render a repo as "name (id)" if its name is known, otherwise just the id.
fn format_repo(repo_id: i64, repo_names: &HashMap<RepositoryId, String>) -> String {
    let name = i32::try_from(repo_id)
        .ok()
        .and_then(|id| repo_names.get(&RepositoryId::new(id)));
    match name {
        Some(name) => format!("{name} ({repo_id})"),
        None => repo_id.to_string(),
    }
}

fn format_optional_timestamp(ts: Option<&Timestamp>) -> String {
    ts.map(format_timestamp).unwrap_or_else(|| "-".to_string())
}

fn format_optional_str(value: Option<&str>) -> &str {
    value.unwrap_or("-")
}

/// Merge raw `(RequestStatus, count)` pairs by their display label,
/// preserving input order. Used so Ready and Polled don't render as two
/// separate "completed" lines.
fn merge_by_label(status_counts: &[(RequestStatus, usize)]) -> Vec<(&'static str, usize)> {
    let mut merged: Vec<(&'static str, usize)> = Vec::new();
    for (status, count) in status_counts {
        let label = status_label(*status);
        if let Some(entry) = merged.iter_mut().find(|(l, _)| *l == label) {
            entry.1 += *count;
        } else {
            merged.push((label, *count));
        }
    }
    merged
}

/// Format a timestamp as a readable UTC datetime string
pub(super) fn format_timestamp(ts: &Timestamp) -> String {
    let dt: DateTime<Utc> = DateTime::from_timestamp(ts.timestamp_seconds(), 0).unwrap_or_default();
    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Format a duration in a human-readable format
pub(super) fn format_duration(duration: &Duration) -> String {
    let total_secs = duration.as_secs();
    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

/// Underscore-separated digit grouping, e.g. 18893 -> "18_893". `num_format`
/// also supports locale-aware separators (e.g. `Locale::en` for commas); we use
/// underscores throughout this view.
static UNDERSCORE_FORMAT: LazyLock<CustomFormat> = LazyLock::new(|| {
    CustomFormat::builder()
        .grouping(Grouping::Standard)
        .separator("_")
        .build()
        .expect("static underscore number format is valid")
});

/// Format a large number with `_` thousands separators, e.g. 18893 -> "18_893".
pub(super) fn format_number(n: usize) -> String {
    let mut buf = Buffer::new();
    buf.write_formatted(&n, &*UNDERSCORE_FORMAT);
    buf.as_str().to_string()
}

/// Truncate `s` (by characters) to at most `max` display columns, appending an
/// ellipsis if it had to be shortened.
fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

/// Render the "Repo Names" column for a backfill list row: the repo names,
/// truncated to a reasonable width. Unknown ids fall back to the bare id.
/// The repo count lives in its own column, so it isn't repeated here.
fn format_repo_names_cell(repo_ids: &[i64], repo_names: &HashMap<i64, String>) -> String {
    /// Max width of the names portion before we truncate with an ellipsis.
    const MAX_NAMES_WIDTH: usize = 70;

    let name_of = |repo_id: i64| -> String {
        match repo_names.get(&repo_id) {
            // Fall back to the id for unknown or empty names so we never render
            // a blank entry.
            Some(name) if !name.is_empty() => name.clone(),
            _ => format_number(repo_id.max(0) as usize),
        }
    };

    // Resolve and sort names for stable output.
    let mut names: Vec<String> = repo_ids.iter().map(|id| name_of(*id)).collect();
    names.sort();

    match names.as_slice() {
        [] => "-".to_string(),
        [single] => truncate_str(single, MAX_NAMES_WIDTH),
        _ => {
            let mut shown: Vec<&str> = Vec::new();
            let mut width = 0usize;
            for name in &names {
                let sep = if shown.is_empty() { 0 } else { ", ".len() };
                if width + sep + name.chars().count() > MAX_NAMES_WIDTH {
                    break;
                }
                width += sep + name.chars().count();
                shown.push(name);
            }
            if shown.is_empty() {
                // Even the first name doesn't fit — show a truncated version so
                // the cell isn't blank.
                truncate_str(&names[0], MAX_NAMES_WIDTH)
            } else if shown.len() < names.len() {
                format!("{}, …", shown.join(", "))
            } else {
                shown.join(", ")
            }
        }
    }
}

/// Display a list of recent backfill jobs. `repo_names` maps repo id to name
/// for every repo referenced in the list (config repos plus git repos resolved
/// from the source-of-truth table).
pub(super) fn display_backfill_list(
    backfills: &[BackfillListRow],
    repo_names: &HashMap<i64, String>,
    all_active: bool,
) {
    if all_active {
        println!("\nAvailable Backfill Jobs (all active)");
    } else {
        println!("\nAvailable Backfill Jobs (recent)");
    }
    println!("{}", "━".repeat(80));

    let mut table = Table::new();
    table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);

    table.set_titles(Row::new(vec![
        Cell::new("Request ID"),
        Cell::new("Created At"),
        Cell::new("Submitted By"),
        Cell::new("Status"),
        Cell::new("Type"),
        Cell::new("Repos"),
        Cell::new("Repo Names"),
    ]));

    for row in backfills {
        let status = if row.aggregate_status == RepoStatus::InProgress && row.has_failed_requests {
            format!("{}*", row.aggregate_status)
        } else {
            row.aggregate_status.to_string()
        };
        table.add_row(Row::new(vec![
            Cell::new(&row.request_id.0.to_string()),
            Cell::new(&format_timestamp(&row.created_at)),
            Cell::new(row.created_by.as_deref().unwrap_or("-")),
            Cell::new(&status),
            Cell::new(row.derived_data_type.as_deref().unwrap_or("-")),
            Cell::new(&format_number(row.repo_count.max(0) as usize)),
            Cell::new(&format_repo_names_cell(&row.repo_ids, repo_names)),
        ]));
    }

    table.printstd();
    println!("{}", "━".repeat(80));
    if backfills
        .iter()
        .any(|row| row.aggregate_status == RepoStatus::InProgress && row.has_failed_requests)
    {
        println!("\n* status has failed requests but still has pending or in-progress work.");
    }
    println!("\nUse --request-id <ID> to see detailed progress for a specific backfill.");
}

/// Display summary for a multi-repo backfill
pub(super) fn display_multi_repo_summary(
    data: &BackfillDisplayData,
    total_repos: usize,
    repos_by_status: &[(String, usize)],
    failed_repos: &[(i64, usize)],
    repo_names: &HashMap<RepositoryId, String>,
) {
    println!("\nBackfill Status for Request ID: {}", data.request_id.0);
    println!("{}", "━".repeat(80));
    println!();

    println!("Root Request Details:");
    println!("  Request ID:        {}", data.request_id.0);
    println!(
        "  Created At:        {}",
        format_timestamp(&data.created_at)
    );
    println!(
        "  Submitted By:      {}",
        data.created_by.as_deref().unwrap_or("-")
    );
    println!("  Status:            {}", data.aggregate_status);
    println!("  Request Type:      {}", data.request_type);
    println!(
        "  Derived Data Type: {}",
        data.derived_data_type.as_deref().unwrap_or("-")
    );
    println!();

    if let Some(settings) = &data.settings {
        print_settings_section(settings);
    }

    println!("Overall Progress:");
    println!("  Total Repos:         {}", format_number(total_repos));
    println!(
        "  Total Requests:      {}",
        format_number(data.total_requests)
    );
    for (label, count) in merge_by_label(&data.status_counts) {
        let percentage = (count as f64 / data.total_requests as f64) * 100.0;
        println!(
            "  {:<18} {} ({:.1}%)",
            format!("{}:", label),
            format_number(count),
            percentage
        );
    }
    println!();

    print_timing_section(
        &data.elapsed_time,
        data.avg_duration.as_ref(),
        data.requests_per_hour,
        data.estimated_remaining.as_ref(),
    );

    println!("Repository Status Summary:");
    for (status_label, count) in repos_by_status {
        let percentage = (*count as f64 / total_repos as f64) * 100.0;
        println!(
            "  {:<18} {} repos ({:.1}%)",
            format!("{}:", status_label),
            format_number(*count),
            percentage
        );
    }
    println!();

    print_type_breakdown_table(&data.status_counts, &data.type_breakdown);

    if !failed_repos.is_empty() {
        println!();
        println!("Failed Repos ({}):", failed_repos.len());
        for (repo_id, failed_count) in failed_repos {
            println!(
                "  - Repo {}: {} failed requests",
                format_repo(*repo_id, repo_names),
                failed_count
            );
        }
        println!();
        println!(
            "Use -R <REPO> --request-id {} to drill down into a specific repo.",
            data.request_id.0
        );
    }
}

/// Display detailed progress for a specific repo in a multi-repo backfill
pub(super) fn display_repo_detail(data: &RepoDisplayData) {
    let repo_label = match &data.repo_name {
        Some(name) => format!("{} ({})", name, data.repo_id),
        None => data.repo_id.to_string(),
    };
    println!(
        "\nBackfill Status for Request ID: {}, Repo: {}",
        data.request_id.0, repo_label,
    );
    println!("{}", "━".repeat(80));
    println!();

    println!("Repo Details:");
    println!("  Repo:              {repo_label}");
    println!("  Overall Status:    {}", data.overall_status);
    println!(
        "  Derived Data Type: {}",
        data.derived_data_type.as_deref().unwrap_or("-")
    );
    println!();

    print_progress_section(data.total_requests, &data.status_counts);
    print_type_breakdown_table(&data.status_counts, &data.type_breakdown);
}

/// Display an individual derive_boundaries or derive_slice request.
pub(super) fn display_child_request_detail(
    data: &BackfillChildDisplayData,
    repo_names: &HashMap<RepositoryId, String>,
) {
    println!("\nBackfill Child Request: {}", data.entry.id.0);
    println!("{}", "━".repeat(80));
    println!();

    println!("Request Details:");
    println!("  Request ID:        {}", data.entry.id.0);
    println!("  Request Type:      {}", data.entry.request_type);
    println!("  Status:            {}", data.entry.status);
    if let Some(root_request_id) = data.entry.root_request_id {
        println!("  Root Request ID:   {}", root_request_id.0);
    } else {
        println!("  Root Request ID:   -");
    }
    if let Some(repo_id) = data.entry.repo_id {
        println!(
            "  Repo:              {}",
            format_repo(repo_id.id() as i64, repo_names)
        );
    } else {
        println!("  Repo:              -");
    }
    println!(
        "  Created At:        {}",
        format_timestamp(&data.entry.created_at)
    );
    println!(
        "  Started At:        {}",
        format_optional_timestamp(data.entry.started_processing_at.as_ref())
    );
    println!(
        "  Last Heartbeat:    {}",
        format_optional_timestamp(data.entry.inprogress_last_updated_at.as_ref())
    );
    println!(
        "  Ready At:          {}",
        format_optional_timestamp(data.entry.ready_at.as_ref())
    );
    println!(
        "  Failed At:         {}",
        format_optional_timestamp(data.entry.failed_at.as_ref())
    );
    println!(
        "  Polled At:         {}",
        format_optional_timestamp(data.entry.polled_at.as_ref())
    );
    println!(
        "  Claimed By:        {}",
        data.entry
            .claimed_by
            .as_ref()
            .map(|c| c.0.as_str())
            .unwrap_or("-")
    );
    println!(
        "  Retries:           {}",
        data.entry
            .num_retries
            .map(|n| n.to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    println!(
        "  Submitted By:      {}",
        format_optional_str(data.entry.created_by.as_deref())
    );
    println!("  Args Blob:         {}", data.entry.args_blobstore_key);
    println!(
        "  Result Blob:       {}",
        data.entry
            .result_blobstore_key
            .as_ref()
            .map(|key| key.0.as_str())
            .unwrap_or("-")
    );
    println!();

    match &data.params {
        BackfillChildParams::DeriveBoundaries {
            repo_id,
            derived_data_type,
            boundary_cs_ids,
            concurrency,
            use_predecessor_derivation,
            config_name,
        } => {
            println!("Derive Boundaries Params:");
            println!("  Repo:              {}", format_repo(*repo_id, repo_names));
            println!("  Derived Data Type: {derived_data_type}");
            println!(
                "  Config Name:       {}",
                format_optional_str(config_name.as_deref())
            );
            println!("  Concurrency:       {concurrency}");
            println!("  Use Predecessor:   {use_predecessor_derivation}");
            println!(
                "  Boundary Count:    {}",
                format_number(boundary_cs_ids.len())
            );
            if let Some(boundary_derivation_status) = &data.boundary_derivation_status {
                match boundary_derivation_status {
                    BoundaryDerivationStatus::Checked {
                        already_derived_count,
                        not_derived_count,
                    } => {
                        let checked_count = already_derived_count + not_derived_count;
                        let derived_percentage = if checked_count == 0 {
                            0.0
                        } else {
                            (*already_derived_count as f64 / checked_count as f64) * 100.0
                        };
                        println!(
                            "  Derived:           {}/{} ({:.1}%)",
                            format_number(*already_derived_count),
                            format_number(checked_count),
                            derived_percentage
                        );
                    }
                    BoundaryDerivationStatus::NotChecked { reason } => {
                        println!("  Derived Check:     not checked ({reason})");
                    }
                }
            }
            println!("  Boundary Changesets:");
            for cs_id in boundary_cs_ids {
                println!("    {cs_id}");
            }
        }
        BackfillChildParams::DeriveSlice {
            repo_id,
            derived_data_type,
            segments,
            config_name,
        } => {
            println!("Derive Slice Params:");
            println!("  Repo:              {}", format_repo(*repo_id, repo_names));
            println!("  Derived Data Type: {derived_data_type}");
            println!(
                "  Config Name:       {}",
                format_optional_str(config_name.as_deref())
            );
            println!("  Segment Count:     {}", format_number(segments.len()));
            println!("  Segments:");
            for (idx, segment) in segments.iter().enumerate() {
                println!("    {}. head: {}", idx + 1, segment.head);
                println!("       base: {}", segment.base);
            }
        }
    }

    if let Some(result) = &data.result {
        println!();
        println!("Result:");
        match result {
            BackfillChildResult::DeriveBoundaries {
                derived_count,
                error_message,
            }
            | BackfillChildResult::DeriveSlice {
                derived_count,
                error_message,
            } => {
                println!("  Derived Count:     {derived_count}");
                println!(
                    "  Error Message:     {}",
                    format_optional_str(error_message.as_deref())
                );
            }
            BackfillChildResult::Error { message } => {
                println!("  Error Message:     {message}");
            }
        }
    }
}

fn print_progress_section(total_requests: usize, status_counts: &[(RequestStatus, usize)]) {
    println!("Overall Progress:");
    println!("  Total Requests:      {}", format_number(total_requests));

    for (label, count) in merge_by_label(status_counts) {
        let percentage = (count as f64 / total_requests as f64) * 100.0;
        println!(
            "  {:<18} {} ({:.1}%)",
            format!("{}:", label),
            format_number(count),
            percentage
        );
    }
    println!();
}

fn print_settings_section(settings: &BackfillSettings) {
    println!("Settings:");
    println!(
        "  Slice Size:           {}",
        format_number(settings.slice_size.max(0) as usize)
    );
    println!(
        "  Boundaries Concurrency: {}",
        settings.boundaries_concurrency
    );
    println!(
        "  Num Boundary Requests:  {}",
        settings.num_boundary_requests
    );
    println!("  Reslice:              {}", settings.reslice);
    println!(
        "  Config Name:          {}",
        format_optional_str(settings.config_name.as_deref())
    );
    println!();
}

fn print_timing_section(
    elapsed_time: &Duration,
    avg_duration: Option<&Duration>,
    requests_per_hour: f64,
    estimated_remaining: Option<&Duration>,
) {
    println!("Performance Metrics:");
    println!("  Elapsed Time:        {}", format_duration(elapsed_time));
    if let Some(avg) = avg_duration {
        println!(
            "  Avg Duration:        {} per request",
            format_duration(avg)
        );
    }
    if requests_per_hour > 0.0 {
        // Show decimals: at whole-number resolution a slow backfill rounds to
        // an unhelpful 0 or 1 requests/hour.
        println!("  Completion Rate:     {requests_per_hour:.2} requests/hour");
    }
    if let Some(est) = estimated_remaining {
        println!("  Est. Remaining:      ~{}", format_duration(est));
    }
    println!();
}

fn print_type_breakdown_table(
    status_counts: &[(RequestStatus, usize)],
    type_breakdown: &[(String, Vec<(RequestStatus, usize)>)],
) {
    println!("Breakdown by Request Type:");
    println!("{}", "━".repeat(80));

    let mut table = Table::new();
    table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);

    // Use the merged display labels as column ordering — distinct underlying
    // statuses that share a label (Ready/Polled both → "completed") collapse
    // into a single column.
    let header_labels: Vec<&'static str> = merge_by_label(status_counts)
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    let mut header_cells = vec![Cell::new("Request Type")];
    for label in &header_labels {
        header_cells.push(Cell::new(label));
    }
    table.set_titles(Row::new(header_cells));

    for (request_type, statuses) in type_breakdown {
        let mut row_cells = vec![Cell::new(request_type)];
        for header_label in &header_labels {
            let count: usize = statuses
                .iter()
                .filter(|(s, _)| status_label(*s) == *header_label)
                .map(|(_, c)| *c)
                .sum();
            row_cells.push(Cell::new(&format_number(count)));
        }
        table.add_row(Row::new(row_cells));
    }

    table.printstd();
    println!("{}", "━".repeat(80));
}

/// Display a per-repo table showing progress for each repository in a multi-repo backfill.
pub(super) fn display_repo_detail_table(rows: &mut [RepoDetailRow]) {
    fn status_sort_key(status: &RepoStatus) -> u8 {
        match status {
            RepoStatus::Failed => 0,
            RepoStatus::InProgress => 1,
            RepoStatus::NotStarted => 2,
            RepoStatus::Completed => 3,
        }
    }

    rows.sort_by(|a, b| {
        status_sort_key(&a.status)
            .cmp(&status_sort_key(&b.status))
            .then_with(|| {
                let a_name = a.repo_name.as_deref().unwrap_or("");
                let b_name = b.repo_name.as_deref().unwrap_or("");
                a_name.cmp(b_name)
            })
            .then_with(|| a.repo_id.cmp(&b.repo_id))
    });

    println!();
    println!("Per-Repository Details:");
    println!("{}", "━".repeat(80));

    let mut table = Table::new();
    table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);

    table.set_titles(Row::new(vec![
        Cell::new("Repository"),
        Cell::new("Status"),
        Cell::new("Derived"),
        Cell::new("Total"),
    ]));

    for row in rows.iter() {
        let repo_label = match &row.repo_name {
            Some(name) => format!("{} ({})", name, row.repo_id),
            None => row.repo_id.to_string(),
        };
        table.add_row(Row::new(vec![
            Cell::new(&repo_label),
            Cell::new(&row.status.to_string()),
            Cell::new(&format_number(row.derived)),
            Cell::new(&format_number(row.total)),
        ]));
    }

    table.printstd();
    println!("{}", "━".repeat(80));
}

/// Display a per-child-request table for a backfill.
///
/// Large backfills fan out into thousands of `derive_slice` /
/// `derive_boundaries` child requests. Most of them sit in the `new` state
/// waiting to be claimed, so we elide those and just report their count,
/// showing a row per request that has actually been picked up by a worker
/// (along with which worker claimed it, from the `claimed_by` column).
///
/// When `show_repo` is set (multi-repo backfills) a `Repo` column is added so
/// each child request can be attributed to its repository; single-repo
/// backfills omit it since every request belongs to the same repo.
pub(super) fn display_child_request_table(
    rows: &mut [ChildRequestRow],
    new_count: usize,
    show_repo: bool,
    repo_names: &HashMap<RepositoryId, String>,
) {
    fn status_sort_key(status: RequestStatus) -> u8 {
        match status {
            RequestStatus::Failed => 0,
            RequestStatus::InProgress => 1,
            RequestStatus::New => 2,
            RequestStatus::Ready | RequestStatus::Polled => 3,
        }
    }

    rows.sort_by(|a, b| {
        status_sort_key(a.status)
            .cmp(&status_sort_key(b.status))
            .then_with(|| a.repo_id.cmp(&b.repo_id))
            .then_with(|| a.id.cmp(&b.id))
    });

    println!();
    println!("Child Requests:");
    println!("{}", "━".repeat(80));

    let mut table = Table::new();
    table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);

    let mut titles = vec![Cell::new("Request ID")];
    if show_repo {
        titles.push(Cell::new("Repo"));
    }
    titles.extend([
        Cell::new("Type"),
        Cell::new("Status"),
        Cell::new("Claimed By"),
    ]);
    table.set_titles(Row::new(titles));

    for row in rows.iter() {
        let mut cells = vec![Cell::new(&row.id.to_string())];
        if show_repo {
            let repo = row
                .repo_id
                .map(|id| format_repo(id, repo_names))
                .unwrap_or_else(|| "-".to_string());
            cells.push(Cell::new(&repo));
        }
        cells.extend([
            Cell::new(&row.request_type),
            Cell::new(status_label(row.status)),
            Cell::new(row.claimed_by.as_deref().unwrap_or("-")),
        ]);
        table.add_row(Row::new(cells));
    }

    table.printstd();
    println!("{}", "━".repeat(80));
    if new_count > 0 {
        println!(
            "... and {} not-yet-started (new) request{} not shown",
            format_number(new_count),
            if new_count == 1 { "" } else { "s" }
        );
    }
}

/// Display detailed progress for a single-repo backfill
pub(super) fn display_single_repo_detail(
    data: &BackfillDisplayData,
    repo_id: Option<i64>,
    repo_names: &HashMap<RepositoryId, String>,
) {
    println!("\nBackfill Status for Request ID: {}", data.request_id.0);
    println!("{}", "━".repeat(80));
    println!();

    println!("Root Request Details:");
    println!("  Request ID:        {}", data.request_id.0);
    println!(
        "  Created At:        {}",
        format_timestamp(&data.created_at)
    );
    println!(
        "  Submitted By:      {}",
        data.created_by.as_deref().unwrap_or("-")
    );
    println!("  Status:            {}", data.aggregate_status);
    if let Some(repo_id) = repo_id {
        println!("  Repo:              {}", format_repo(repo_id, repo_names));
    }
    println!("  Request Type:      {}", data.request_type);
    println!(
        "  Derived Data Type: {}",
        data.derived_data_type.as_deref().unwrap_or("-")
    );
    println!();

    if let Some(settings) = &data.settings {
        print_settings_section(settings);
    }

    print_progress_section(data.total_requests, &data.status_counts);
    print_timing_section(
        &data.elapsed_time,
        data.avg_duration.as_ref(),
        data.requests_per_hour,
        data.estimated_remaining.as_ref(),
    );
    print_type_breakdown_table(&data.status_counts, &data.type_breakdown);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(12345), "12_345");
    }

    #[test]
    fn test_format_duration() {
        // 12345s = 3h 25m 45s; shown as "{h}h {m}m" once hours are non-zero.
        assert_eq!(format_duration(&Duration::from_secs(12345)), "3h 25m");
        // Over 24h switches to "{d}d {h}h": 200000s = 2d 7h 33m 20s.
        assert_eq!(format_duration(&Duration::from_secs(200000)), "2d 7h");
        // Exactly 24h: 1d 0h.
        assert_eq!(format_duration(&Duration::from_secs(86400)), "1d 0h");
    }

    #[test]
    fn test_format_timestamp() {
        assert_eq!(
            format_timestamp(&Timestamp::from_timestamp_secs(12345)),
            "1970-01-01 03:25:45 UTC"
        );
    }

    #[test]
    fn test_truncate_str() {
        // Shorter than the budget: unchanged.
        assert_eq!(truncate_str("12345", 70), "12345");
        // Longer than the budget: truncated to `max` columns with an ellipsis.
        assert_eq!(truncate_str("12345", 4), "123…");
    }

    #[test]
    fn test_format_repo_names_cell_single_unknown_id() {
        // An id with no known name falls back to the bare grouped id.
        let names = HashMap::new();
        assert_eq!(format_repo_names_cell(&[12345], &names), "12_345");
    }

    #[test]
    fn test_format_repo_names_cell_single_known_name() {
        let names = HashMap::from([(12345, "fbsource".to_string())]);
        assert_eq!(format_repo_names_cell(&[12345], &names), "fbsource");
    }
}

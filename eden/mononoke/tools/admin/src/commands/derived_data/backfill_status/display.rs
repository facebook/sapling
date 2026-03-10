/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(dead_code)]

use std::time::Duration;

use chrono::DateTime;
use chrono::Utc;
use mononoke_types::Timestamp;
use prettytable::Cell;
use prettytable::Row;
use prettytable::Table;
use prettytable::format;
use requests_table::RequestStatus;
use requests_table::RowId;

use super::types::BackfillDisplayData;
use super::types::RepoDisplayData;

/// Format a timestamp as a readable UTC datetime string
pub(super) fn format_timestamp(ts: &Timestamp) -> String {
    let dt: DateTime<Utc> = DateTime::from_timestamp(ts.timestamp_seconds(), 0).unwrap_or_default();
    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Format a duration in a human-readable format
pub(super) fn format_duration(duration: &Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

/// Format a large number with thousands separators
pub(super) fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }
    result
}

/// Display a list of recent backfill jobs
pub(super) fn display_backfill_list(backfills: Vec<(RowId, Timestamp, RequestStatus, i64)>) {
    println!("\nAvailable Backfill Jobs (recent)");
    println!("{}", "━".repeat(80));

    let mut table = Table::new();
    table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);

    table.set_titles(Row::new(vec![
        Cell::new("Request ID"),
        Cell::new("Created At"),
        Cell::new("Status"),
        Cell::new("Repos"),
    ]));

    for (request_id, created_at, status, repo_count) in backfills {
        table.add_row(Row::new(vec![
            Cell::new(&request_id.0.to_string()),
            Cell::new(&format_timestamp(&created_at)),
            Cell::new(&status.to_string()),
            Cell::new(&repo_count.to_string()),
        ]));
    }

    table.printstd();
    println!("{}", "━".repeat(80));
    println!("\nUse --request-id <ID> to see detailed progress for a specific backfill.");
}

/// Display summary for a multi-repo backfill
pub(super) fn display_multi_repo_summary(
    data: &BackfillDisplayData,
    total_repos: usize,
    repos_by_status: &[(String, usize)],
    failed_repos: &[(i64, usize)],
) {
    println!("\nBackfill Status for Request ID: {}", data.request_id.0);
    println!("{}", "━".repeat(80));
    println!();

    println!("Root Request Details:");
    println!("  Request ID:     {}", data.request_id.0);
    println!("  Created At:     {}", format_timestamp(&data.created_at));
    println!("  Status:         {}", data.status);
    println!("  Request Type:   {}", data.request_type);
    println!();

    println!("Overall Progress:");
    println!("  Total Repos:         {}", format_number(total_repos));
    println!(
        "  Total Requests:      {}",
        format_number(data.total_requests)
    );
    for (status, count) in &data.status_counts {
        let percentage = (*count as f64 / data.total_requests as f64) * 100.0;
        println!(
            "  {:<18} {} ({:.1}%)",
            format!("{}:", status),
            format_number(*count),
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
            println!("  - Repo {}: {} failed requests", repo_id, failed_count);
        }
        println!();
        println!(
            "Use --request-id {} --repo-id <ID> to drill down into a specific repo.",
            data.request_id.0
        );
    }
}

/// Display detailed progress for a specific repo in a multi-repo backfill
pub(super) fn display_repo_detail(data: &RepoDisplayData) {
    println!(
        "\nBackfill Status for Request ID: {}, Repo ID: {}",
        data.request_id.0, data.repo_id
    );
    println!("{}", "━".repeat(80));
    println!();

    println!("Repo Details:");
    println!("  Repo ID:        {}", data.repo_id);
    println!("  Overall Status: {}", data.overall_status);
    println!();

    print_progress_section(data.total_requests, &data.status_counts);
    print_type_breakdown_table(&data.status_counts, &data.type_breakdown);
}

fn print_progress_section(total_requests: usize, status_counts: &[(RequestStatus, usize)]) {
    println!("Overall Progress:");
    println!("  Total Requests:      {}", format_number(total_requests));

    for (status, count) in status_counts {
        let percentage = (*count as f64 / total_requests as f64) * 100.0;
        println!(
            "  {:<18} {} ({:.1}%)",
            format!("{}:", status),
            format_number(*count),
            percentage
        );
    }
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
        println!(
            "  Completion Rate:     {:.0} requests/hour",
            requests_per_hour
        );
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

    let mut header_cells = vec![Cell::new("Request Type")];
    let all_statuses: Vec<String> = status_counts.iter().map(|(s, _)| s.to_string()).collect();
    for status in &all_statuses {
        header_cells.push(Cell::new(status));
    }
    table.set_titles(Row::new(header_cells));

    for (request_type, statuses) in type_breakdown {
        let mut row_cells = vec![Cell::new(request_type)];
        for status_name in &all_statuses {
            let count = statuses
                .iter()
                .find(|(s, _)| s.to_string() == *status_name)
                .map(|(_, c)| *c)
                .unwrap_or(0);
            row_cells.push(Cell::new(&format_number(count)));
        }
        table.add_row(Row::new(row_cells));
    }

    table.printstd();
    println!("{}", "━".repeat(80));
}

/// Display detailed progress for a single-repo backfill
pub(super) fn display_single_repo_detail(data: &BackfillDisplayData, repo_id: Option<i64>) {
    println!("\nBackfill Status for Request ID: {}", data.request_id.0);
    println!("{}", "━".repeat(80));
    println!();

    println!("Root Request Details:");
    println!("  Request ID:     {}", data.request_id.0);
    println!("  Created At:     {}", format_timestamp(&data.created_at));
    println!("  Status:         {}", data.status);
    if let Some(repo_id) = repo_id {
        println!("  Repo:           {}", repo_id);
    }
    println!("  Request Type:   {}", data.request_type);
    println!();

    print_progress_section(data.total_requests, &data.status_counts);
    print_timing_section(
        &data.elapsed_time,
        data.avg_duration.as_ref(),
        data.requests_per_hour,
        data.estimated_remaining.as_ref(),
    );
    print_type_breakdown_table(&data.status_counts, &data.type_breakdown);
}

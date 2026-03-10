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

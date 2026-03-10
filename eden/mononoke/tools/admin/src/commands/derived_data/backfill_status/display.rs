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

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::time::Duration;

use crate::maybe_pad;

pub fn unit_phrase(unit: &str, pos: u64, total: u64) -> String {
    match unit {
        "%" => {
            let total = total.max(1);
            format!("{}%", pos.min(total) * 100 / total)
        }
        "bytes" | "B" => {
            if total == 0 {
                human_bytes(pos as _)
            } else {
                format!("{}/{}", human_bytes(pos as _), human_bytes(total as _))
            }
        }
        _ => {
            if total == 0 {
                if pos == 0 {
                    String::new()
                } else {
                    format!("{}{}", pos, maybe_pad(unit))
                }
            } else {
                format!("{}/{}{}", pos, total, maybe_pad(unit))
            }
        }
    }
}

pub fn human_bytes(bytes: u64) -> String {
    if bytes < 5000 {
        format!("{}B", bytes)
    } else if bytes < 5_000_000 {
        format!("{}KB", bytes / 1000)
    } else if bytes < 5_000_000_000 {
        format!("{}MB", bytes / 1000000)
    } else {
        format!("{}GB", bytes / 1000000000)
    }
}

/// Return short, human readable representation of duration.
/// We try to keep a relatively fixed width of 3 to 5 characters.
pub fn human_duration(d: Duration) -> String {
    let total_secs = d.as_secs();
    if total_secs >= 3600 {
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        format!("{}h{}m", hours, mins)
    } else if total_secs >= 60 {
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{}m{}s", mins, secs)
    } else {
        let sec = d.as_secs_f64();
        if sec < 10_f64 {
            format!("{:.1}s", sec)
        } else {
            format!("{}s", sec.round())
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_human_duration() {
        assert_eq!(human_duration(Duration::from_millis(0)), "0.0s");
        assert_eq!(human_duration(Duration::from_millis(100)), "0.1s");
        assert_eq!(human_duration(Duration::from_millis(1000)), "1.0s");
        assert_eq!(human_duration(Duration::from_millis(9500)), "9.5s");
        assert_eq!(human_duration(Duration::from_secs(12)), "12s");
        assert_eq!(human_duration(Duration::from_millis(12999)), "13s");
        assert_eq!(human_duration(Duration::from_mins(1)), "1m0s");
        assert_eq!(human_duration(Duration::from_secs(80)), "1m20s");
        assert_eq!(human_duration(Duration::from_secs(330)), "5m30s");
        assert_eq!(human_duration(Duration::from_hours(1)), "1h0m");
        assert_eq!(human_duration(Duration::from_secs(3660)), "1h1m");
        assert_eq!(human_duration(Duration::from_hours(10)), "10h0m");
    }
}

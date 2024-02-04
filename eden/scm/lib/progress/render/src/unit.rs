/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
/// We try to keep a relatively fixed width of 3 or 4 characters.
pub fn human_duration(d: Duration) -> String {
    let sec = d.as_secs_f64();
    let (unit_size, unit) = match sec {
        v if v >= 3600_f64 => (3600_f64, "h"),
        v if v >= 60_f64 => (60_f64, "m"),
        _ => (1_f64, "s"),
    };

    if sec / unit_size < 10_f64 {
        format!("{:.1}{}", sec / unit_size, unit)
    } else {
        format!("{}{}", (sec / unit_size).round(), unit)
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
        assert_eq!(human_duration(Duration::from_secs(60)), "1.0m");
        assert_eq!(human_duration(Duration::from_secs(330)), "5.5m");
        assert_eq!(human_duration(Duration::from_secs(3600)), "1.0h");
        assert_eq!(human_duration(Duration::from_secs(36000)), "10h");
    }
}

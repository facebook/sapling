/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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
                    format!("{} {}", pos, unit)
                }
            } else {
                format!("{}/{} {}", pos, total, unit)
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

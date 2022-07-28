/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Utility traits and functions.

use num::PrimInt;

/// Returns `single` if the given number is 1, or `plural` otherwise.
pub(crate) fn plural<'a, T: PrimInt>(n: T, single: &'a str, plural: &'a str) -> &'a str {
    if n == T::one() { single } else { plural }
}

fn byte_count(size: i64, unit_single: &str, unit_plural: &str, multiple: &[&str]) -> String {
    const UNIT_LIMIT: i64 = 9999;
    match (size, multiple.split_last()) {
        (std::i64::MIN..=UNIT_LIMIT, _) | (_, None) => {
            format!("{}{}", size, plural(size, unit_single, unit_plural))
        }
        (size, Some((last_multiple, multiple))) => {
            let mut divisor = 1024;
            for unit in multiple.iter() {
                if size < (UNIT_LIMIT + 1) * divisor {
                    return format!("{:.2}{}", (size as f64) / (divisor as f64), unit);
                }
                divisor *= 1024;
            }
            format!("{:.2}{}", (size as f64) / (divisor as f64), last_multiple)
        }
    }
}

/// Convert a byte count to a human-readable representation of the byte count
/// using appropriate IEC suffixes.
pub(crate) fn byte_count_iec(size: i64) -> String {
    let suffixes = [
        " KiB", " MiB", " GiB", " TiB", " PiB", " EiB", " ZiB", " YiB",
    ];
    byte_count(size, " byte", " bytes", &suffixes)
}

/// Convert a byte count to a human-readable representation of the byte count
/// using short suffixes.
#[allow(dead_code)]
pub(crate) fn byte_count_short(size: i64) -> String {
    byte_count(size, "", "", &["K", "M", "G", "T", "P", "E", "Z", "Y"])
}

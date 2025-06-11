/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod constants;
use std::sync::LazyLock;

pub use constants::VERSION;
pub use constants::VERSION_HASH;

/// Find the YYYYMMDD from a version string. Return YYYYMMDD as int.
fn scan_date_int(version_str: &str) -> i64 {
    let mut current = 0;
    let mut digit_count = 0;

    for c in version_str.chars().chain(std::iter::once('.')) {
        if let Some(digit) = c.to_digit(10) {
            current = current * 10 + digit as i64;
            digit_count += 1;
        } else if digit_count == 8 && looks_like_date(current) {
            break;
        } else {
            current = 0;
            digit_count = 0;
        }
    }

    current
}

fn looks_like_date(mut date_int: i64) -> bool {
    if date_int <= 0 {
        return false;
    }

    let day = date_int % 100;
    if day == 0 || day > 31 {
        return false;
    }

    date_int /= 100;
    let month = date_int % 100;
    if month == 0 || month > 12 {
        return false;
    }

    let year = date_int / 100;
    year >= 2000
}

/// Date in the version number represented as int.
pub static DATE_INT: LazyLock<i64> = LazyLock::new(|| scan_date_int(VERSION));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_date_int() {
        assert_eq!(scan_date_int("dev"), 0);
        assert_eq!(scan_date_int("20231015"), 20231015);
        assert_eq!(scan_date_int("202311111"), 0); // too many digits
        assert_eq!(scan_date_int("020231111"), 0); // too many digits
        assert_eq!(scan_date_int("20231315"), 0); // wrong month
        assert_eq!(scan_date_int("20231000"), 0); // wrong day
        assert_eq!(scan_date_int("10000101"), 0); // year in the past
        assert_eq!(scan_date_int("20231015-123"), 20231015);
        assert_eq!(scan_date_int("v1.2.3-20231015"), 20231015);
        assert_eq!(
            scan_date_int("4.4.2_20250528_160019_128e9d3ea9f0_1.fb.el_128e9d3"),
            20250528
        );
        assert_eq!(scan_date_int("0.2.20250609-151406+89d41e68"), 20250609);
    }
}

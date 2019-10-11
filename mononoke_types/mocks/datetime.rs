/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use chrono::{FixedOffset, TimeZone};
use lazy_static::lazy_static;
use mononoke_types::DateTime;

/// Return a `DateTime` corresponding to <year>-01-01 00:00:00 UTC.
pub fn day_1_utc(year: i32) -> DateTime {
    DateTime::new(FixedOffset::west(0).ymd(year, 1, 1).and_hms(0, 0, 0))
}

/// Return a `DateTime` corresponding to <year>-01-01 00:00:00 UTC,
/// with the specified offset applied.
pub fn day_1_tz(year: i32, offset: i32) -> DateTime {
    DateTime::new(FixedOffset::west(offset).ymd(year, 1, 1).and_hms(0, 0, 0))
}

pub const PST_OFFSET: i32 = 7 * 3600;

lazy_static! {
    /// 1970-01-01 00:00:00 UTC.
    pub static ref EPOCH_ZERO: DateTime = DateTime::from_timestamp(0, 0).unwrap();
    /// 1970-01-01 00:00:00 UTC-07.
    pub static ref EPOCH_ZERO_PST: DateTime = DateTime::from_timestamp(0, PST_OFFSET).unwrap();

    /// 1900-01-01 00:00:00 UTC.
    pub static ref YEAR_1900: DateTime = day_1_utc(1900);
    /// 1900-01-01 00:00:00 UTC-07.
    pub static ref YEAR_1900_PST: DateTime = day_1_tz(1900, PST_OFFSET);

    /// 2000-01-01 00:00:00 UTC.
    pub static ref YEAR_2000: DateTime = day_1_utc(2000);
    /// 2000-01-01 00:00:00 UTC-07.
    pub static ref YEAR_2000_PST: DateTime = day_1_tz(2000, PST_OFFSET);

    /// 2100-01-01 00:00:00 UTC.
    pub static ref YEAR_2100: DateTime = day_1_utc(2000);
    pub static ref YEAR_2100_PST: DateTime = day_1_tz(2100, PST_OFFSET);
}

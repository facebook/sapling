/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::fmt::Display;
use std::str::FromStr;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use chrono::DateTime as ChronoDateTime;
use chrono::Duration as ChronoDuration;
use chrono::FixedOffset;
use chrono::Local;
use chrono::LocalResult;
use chrono::NaiveDateTime;
use chrono::TimeZone;
use chrono_english::parse_date_string;
use chrono_english::Dialect;
use quickcheck::empty_shrinker;
use quickcheck::Arbitrary;
use quickcheck::Gen;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use sql::mysql;

use crate::errors::ErrorKind;
use crate::thrift;

#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    Deserialize,
    Serialize,
    PartialEq,
    PartialOrd
)]
pub struct DateTime(ChronoDateTime<FixedOffset>);

impl DateTime {
    #[inline]
    pub fn new(dt: ChronoDateTime<FixedOffset>) -> Self {
        DateTime(dt)
    }

    pub fn now() -> Self {
        let now = Local::now();
        DateTime(now.with_timezone(now.offset()))
    }

    pub fn from_timestamp(secs: i64, tz_offset_secs: i32) -> Result<Self> {
        let tz = FixedOffset::west_opt(tz_offset_secs).ok_or_else(|| {
            ErrorKind::InvalidDateTime(format!("timezone offset out of range: {}", tz_offset_secs))
        })?;
        let dt = match tz.timestamp_opt(secs, 0) {
            LocalResult::Single(dt) => dt,
            _ => bail!(ErrorKind::InvalidDateTime(format!(
                "seconds out of range: {}",
                secs
            ))),
        };
        Ok(Self::new(dt))
    }

    /// Construct a new `DateTime` from an RFC3339 string.
    ///
    /// RFC3339 is a standardized way to represent a specific moment in time. See
    /// <https://tools.ietf.org/html/rfc3339>.
    pub fn from_rfc3339(rfc3339: &str) -> Result<Self> {
        let dt = ChronoDateTime::parse_from_rfc3339(rfc3339)
            .with_context(|| ErrorKind::InvalidDateTime("while parsing rfc3339".into()))?;
        Ok(Self::new(dt))
    }

    pub fn from_thrift(dt: thrift::DateTime) -> Result<Self> {
        Self::from_timestamp(dt.timestamp_secs, dt.tz_offset_secs)
    }

    /// Retrieves the Unix timestamp in UTC.
    #[inline]
    pub fn timestamp_secs(&self) -> i64 {
        self.0.timestamp()
    }

    /// Retrieves the timezone offset, as represented by the number of seconds to
    /// add to convert local time to UTC.
    #[inline]
    pub fn tz_offset_secs(&self) -> i32 {
        // This is the same as the way Mercurial stores timezone offsets.
        self.0.offset().utc_minus_local()
    }

    #[inline]
    pub fn tz_offset_minutes(&self) -> i16 {
        (self.tz_offset_secs() / 60) as i16
    }

    #[inline]
    pub fn as_chrono(&self) -> &ChronoDateTime<FixedOffset> {
        &self.0
    }

    #[inline]
    pub fn into_chrono(self) -> ChronoDateTime<FixedOffset> {
        self.0
    }

    pub fn into_thrift(self) -> thrift::DateTime {
        thrift::DateTime {
            timestamp_secs: self.timestamp_secs(),
            tz_offset_secs: self.tz_offset_secs(),
        }
    }
}

impl From<DateTime> for ChronoDateTime<FixedOffset> {
    fn from(d: DateTime) -> Self {
        d.into_chrono()
    }
}

impl std::ops::Add<ChronoDuration> for DateTime {
    type Output = DateTime;

    #[inline]
    fn add(self, rhs: ChronoDuration) -> DateTime {
        DateTime::new(self.into_chrono() + rhs)
    }
}

impl std::ops::Sub<ChronoDuration> for DateTime {
    type Output = DateTime;

    #[inline]
    fn sub(self, rhs: ChronoDuration) -> DateTime {
        DateTime::new(self.into_chrono() - rhs)
    }
}

impl Display for DateTime {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}", self.0)
    }
}

impl Arbitrary for DateTime {
    fn arbitrary(g: &mut Gen) -> Self {
        // Ensure a large domain from which to get second values.
        let secs = i32::arbitrary(g) as i64;
        // Timezone offsets in the range [-86399, 86399] (both inclusive) are valid.
        // gen_range generates a value in the range [low, high).
        let tz_offset_secs = (u64::arbitrary(g) % 172_799) as i32 - 86_399;
        DateTime::from_timestamp(secs, tz_offset_secs)
            .expect("Arbitrary instances should always be valid")
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        empty_shrinker()
    }
}

impl FromStr for DateTime {
    type Err = chrono_english::DateError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let now = DateTime::now().into_chrono();
        Ok(DateTime::new(parse_date_string(s, now, Dialect::Us)?))
    }
}

const MS_IN_NS: i64 = 1_000_000;
const SEC_IN_NS: i64 = 1_000_000_000;

/// Number of non-leap-nanoseconds since January 1, 1970 UTC
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[derive(Deserialize, Serialize, mysql::OptTryFromRowField)]
pub struct Timestamp(i64);

impl Timestamp {
    pub fn now() -> Self {
        DateTime::now().into()
    }

    pub fn from_timestamp_secs(ts: i64) -> Self {
        Timestamp(ts * SEC_IN_NS)
    }

    pub fn from_timestamp_nanos(ts: i64) -> Self {
        Timestamp(ts)
    }

    pub fn timestamp_nanos(&self) -> i64 {
        self.0
    }

    pub fn timestamp_seconds(&self) -> i64 {
        self.0 / SEC_IN_NS
    }

    pub fn since_nanos(&self) -> i64 {
        let now = Self::now().timestamp_nanos();
        now - self.0
    }

    pub fn since_millis(&self) -> i64 {
        self.since_nanos() / MS_IN_NS
    }

    pub fn since_seconds(&self) -> i64 {
        self.since_nanos() / SEC_IN_NS
    }
}

impl From<DateTime> for Timestamp {
    fn from(dt: DateTime) -> Self {
        Timestamp(dt.0.timestamp_nanos())
    }
}

impl From<Timestamp> for DateTime {
    fn from(ts: Timestamp) -> Self {
        let ts_secs = ts.timestamp_seconds();
        let ts_nsecs = (ts.0 % SEC_IN_NS) as u32;
        DateTime::new(ChronoDateTime::<FixedOffset>::from_utc(
            NaiveDateTime::from_timestamp(ts_secs, ts_nsecs),
            FixedOffset::west(0),
        ))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use quickcheck::quickcheck;

    quickcheck! {
        fn thrift_roundtrip(dt: DateTime) -> bool {
            let thrift_dt = dt.into_thrift();
            let dt2 = DateTime::from_thrift(thrift_dt)
                .expect("roundtrip instances should always be valid");
            // Equality on DateTime structs doesn't pay attention to the time zone,
            // in order to be consistent with Ord.
            dt == dt2 && dt.tz_offset_secs() == dt2.tz_offset_secs()
        }
    }

    #[test]
    fn rfc3339() {
        // Valid RFC3339 strings.
        DateTime::from_rfc3339("2018-01-01T00:00:00Z").expect("unexpected err - UTC");
        DateTime::from_rfc3339("2018-01-01T00:00:00+04:00").expect("unexpected err - +04:00");
        DateTime::from_rfc3339("2018-01-01T00:00:00-04:00").expect("unexpected err - -04:00");
        DateTime::from_rfc3339("2018-01-01T01:02:03.04+05:45").expect("unexpected err - subsecond");

        // Missing information.
        DateTime::from_rfc3339("2018-01-01").expect_err("unexpected Ok - no time");
        DateTime::from_rfc3339("12:23:36").expect_err("unexpected Ok - no date");
        DateTime::from_rfc3339("2018-01-01T12:23").expect_err("unexpected Ok - no seconds");
        DateTime::from_rfc3339("2018-01-01T12:23:36").expect_err("unexpected Ok - no timezone");
    }

    #[test]
    fn bad_inputs() {
        DateTime::from_timestamp(0, 86_400)
            .expect_err("unexpected OK - tz_offset_secs out of bounds");
        DateTime::from_timestamp(0, -86_400)
            .expect_err("unexpected OK - tz_offset_secs out of bounds");
        DateTime::from_timestamp(i64::min_value(), 0)
            .expect_err("unexpected OK - timestamp_secs out of bounds");
        DateTime::from_timestamp(i64::max_value(), 0)
            .expect_err("unexpected OK - timestamp_secs out of bounds");
    }

    #[test]
    fn bad_thrift() {
        DateTime::from_thrift(thrift::DateTime {
            timestamp_secs: 0,
            tz_offset_secs: 86_400,
        })
        .expect_err("unexpected OK - tz_offset_secs out of bounds");
        DateTime::from_thrift(thrift::DateTime {
            timestamp_secs: 0,
            tz_offset_secs: -86_400,
        })
        .expect_err("unexpected OK - tz_offset_secs out of bounds");
        DateTime::from_thrift(thrift::DateTime {
            timestamp_secs: i64::min_value(),
            tz_offset_secs: 0,
        })
        .expect_err("unexpected OK - timestamp_secs out of bounds");
        DateTime::from_thrift(thrift::DateTime {
            timestamp_secs: i64::max_value(),
            tz_offset_secs: 0,
        })
        .expect_err("unexpected OK - timestamp_secs out of bounds");
    }

    #[test]
    fn timestamp_round_trip() {
        let ts0 = Timestamp::now();
        let dt0: DateTime = ts0.into();
        let ts1: Timestamp = dt0.into();
        let dt1: DateTime = ts1.into();
        assert_eq!(ts0, ts1);
        assert_eq!(dt0, dt1);
    }

    #[test]
    fn seconds() {
        let ts0 = Timestamp::from_timestamp_nanos(SEC_IN_NS);
        let ts1 = Timestamp::from_timestamp_secs(1);
        assert_eq!(ts0, ts1);
    }
}

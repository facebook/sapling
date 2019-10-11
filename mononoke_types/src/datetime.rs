/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::fmt::{self, Display};

use chrono::{
    DateTime as ChronoDateTime, FixedOffset, Local, LocalResult, NaiveDateTime, TimeZone,
};
use failure_ext::bail_err;
use quickcheck::{empty_shrinker, Arbitrary, Gen};
use serde_derive::{Deserialize, Serialize};

use crate::errors::*;
use crate::thrift;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, Serialize, PartialEq, PartialOrd)]
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
            _ => bail_err!(ErrorKind::InvalidDateTime(format!(
                "seconds out of range: {}",
                secs
            ))),
        };
        Ok(Self::new(dt))
    }

    /// Construct a new `DateTime` from an RFC3339 string.
    ///
    /// RFC3339 is a standardized way to represent a specific moment in time. See
    /// https://tools.ietf.org/html/rfc3339.
    pub fn from_rfc3339(rfc3339: &str) -> Result<Self> {
        let dt = ChronoDateTime::parse_from_rfc3339(rfc3339)
            .with_context(|_| ErrorKind::InvalidDateTime("while parsing rfc3339".into()))?;
        Ok(Self::new(dt))
    }

    pub(crate) fn from_thrift(dt: thrift::DateTime) -> Result<Self> {
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

    pub(crate) fn into_thrift(self) -> thrift::DateTime {
        thrift::DateTime {
            timestamp_secs: self.timestamp_secs(),
            tz_offset_secs: self.tz_offset_secs(),
        }
    }
}

impl Display for DateTime {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}", self.0)
    }
}

impl Arbitrary for DateTime {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        // Ensure a large domain from which to get second values.
        let secs = g.gen_range(i32::min_value(), i32::max_value()) as i64;
        // Timezone offsets in the range [-86399, 86399] (both inclusive) are valid.
        // gen_range generates a value in the range [low, high).
        let tz_offset_secs = g.gen_range(-86_399, 86_400);
        DateTime::from_timestamp(secs, tz_offset_secs)
            .expect("Arbitrary instances should always be valid")
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        empty_shrinker()
    }
}

/// Number of non-leap-nanoseconds since January 1, 1970 UTC
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Ord,
    PartialOrd,
    Deserialize,
    Serialize
)]
pub struct Timestamp(i64);

impl Timestamp {
    pub fn now() -> Self {
        DateTime::now().into()
    }

    pub fn from_timestamp_nanos(ts: i64) -> Self {
        Timestamp(ts)
    }

    pub fn timestamp_nanos(&self) -> i64 {
        self.0
    }

    pub fn timestamp_seconds(&self) -> i64 {
        self.0 / 1_000_000_000
    }

    pub fn since_nanos(&self) -> i64 {
        let now = Self::now().timestamp_nanos();
        now - self.0
    }

    pub fn since_seconds(&self) -> i64 {
        self.since_nanos() / 1_000_000_000
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
        let ts_nsecs = (ts.0 % 1_000_000_000) as u32;
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
}

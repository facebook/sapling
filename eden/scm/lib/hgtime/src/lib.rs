/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # parsedate
//!
//! See [`HgTime`] and [`HgTime::parse`] for main features.

use chrono::prelude::*;
use chrono::{Duration, LocalResult};
use std::convert::{TryFrom, TryInto};
use std::ops::{Add, Range, RangeInclusive, Sub};
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

/// A simple time structure that matches hg's time representation.
///
/// Internally it's unixtime (in GMT), and offset (GMT -1 = +3600).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HgTime {
    pub unixtime: i64,
    pub offset: i32,
}

const DEFAULT_FORMATS: [&str; 35] = [
    // mercurial/util.py defaultdateformats
    "%Y-%m-%dT%H:%M:%S", // the 'real' ISO8601
    "%Y-%m-%dT%H:%M",    //   without seconds
    "%Y-%m-%dT%H%M%S",   // another awful but legal variant without :
    "%Y-%m-%dT%H%M",     //   without seconds
    "%Y-%m-%d %H:%M:%S", // our common legal variant
    "%Y-%m-%d %H:%M",    //   without seconds
    "%Y-%m-%d %H%M%S",   // without :
    "%Y-%m-%d %H%M",     //   without seconds
    "%Y-%m-%d %I:%M:%S%p",
    "%Y-%m-%d %H:%M",
    "%Y-%m-%d %I:%M%p",
    "%a %b %d %H:%M:%S %Y",
    "%a %b %d %I:%M:%S%p %Y",
    "%a, %d %b %Y %H:%M:%S", //  GNU coreutils "/bin/date --rfc-2822"
    "%b %d %H:%M:%S %Y",
    "%b %d %I:%M:%S%p %Y",
    "%b %d %H:%M:%S",
    "%b %d %I:%M:%S%p",
    "%b %d %H:%M",
    "%b %d %I:%M%p",
    "%m-%d",
    "%m/%d",
    "%Y-%m-%d",
    "%m/%d/%y",
    "%m/%d/%Y",
    "%b",
    "%b %d",
    "%b %Y",
    "%b %d %Y",
    "%I:%M%p",
    "%H:%M",
    "%H:%M:%S",
    "%I:%M:%S%p",
    "%Y",
    "%Y-%m",
];

const INVALID_OFFSET: i32 = i32::max_value();
static DEFAULT_OFFSET: AtomicI32 = AtomicI32::new(INVALID_OFFSET);
static FORCED_NOW: AtomicU64 = AtomicU64::new(0); // test only

impl HgTime {
    /// Supported Range. This is to be compatible with Python stdlib.
    ///
    /// The Python `datetime`  library can only express a limited range
    /// of dates (0001-01-01 to 9999-12-31). Its strftime requires
    /// year >= 1900.
    pub const RANGE: RangeInclusive<HgTime> = Self::min_value()..=Self::max_value();

    /// Return the current time, or `None` if the timestamp is outside
    /// [`HgTime::RANGE`].
    pub fn now() -> Option<Self> {
        let forced_now = FORCED_NOW.load(Ordering::SeqCst);
        if forced_now == 0 {
            Local::now()
                .try_into()
                .ok()
                .map(|t: HgTime| t.use_default_offset())
                .and_then(|t| t.bounded())
        } else {
            Some(Self::from_compact_u64(forced_now))
        }
    }

    pub fn to_local(self) -> DateTime<Local> {
        DateTime::from(self.to_utc())
    }

    pub fn to_utc(self) -> DateTime<Utc> {
        DateTime::from_utc(self.to_naive(), Utc)
    }

    fn to_naive(self) -> NaiveDateTime {
        NaiveDateTime::from_timestamp(self.unixtime, 0)
    }

    /// Set as the faked "now". Useful for testing.
    ///
    /// This should only be used for testing.
    pub fn set_as_now_for_testing(self) {
        FORCED_NOW.store(self.to_lossy_compact_u64(), Ordering::SeqCst);
    }

    /// Parse a date string.
    ///
    /// Return `None` if it cannot be parsed.
    ///
    /// This function matches `mercurial.util.parsedate`, and can parse
    /// some additional forms like `2 days ago`.
    pub fn parse(date: &str) -> Option<Self> {
        match date {
            "now" => Self::now(),
            "today" => Self::now().and_then(|now| {
                Self::try_from(now.to_local().date().and_hms(0, 0, 0))
                    .ok()
                    .map(|t| t.use_default_offset())
            }),
            "yesterday" => Self::now().and_then(|now| {
                Self::try_from(now.to_local().date().and_hms(0, 0, 0) - Duration::days(1))
                    .ok()
                    .map(|t| t.use_default_offset())
            }),
            date if date.ends_with(" ago") => {
                let duration_str = &date[..date.len() - 4];
                duration_str
                    .parse::<humantime::Duration>()
                    .ok()
                    .and_then(|duration| Self::now().and_then(|n| n - duration.as_secs()))
            }
            _ => Self::parse_absolute(date, default_date_lower),
        }
    }

    /// Parse a date string as a range.
    ///
    /// For example, `Apr 2000` covers range `Apr 1, 2000` to `Apr 30, 2000`.
    /// Also support more explicit ranges:
    /// - START to END
    /// - > START
    /// - < END
    pub fn parse_range(date: &str) -> Option<Range<Self>> {
        Self::parse_range_internal(date, true)
    }

    fn parse_range_internal(date: &str, support_to: bool) -> Option<Range<Self>> {
        match date {
            "now" => Self::now().and_then(|n| (n + 1).map(|m| n..m)),
            "today" => Self::now().and_then(|now| {
                let date = now.to_local().date();
                let start = Self::try_from(date.and_hms(0, 0, 0)).map(|t| t.use_default_offset());
                let end =
                    Self::try_from(date.and_hms(23, 59, 59)).map(|t| t.use_default_offset() + 1);
                if let (Ok(start), Ok(Some(end))) = (start, end) {
                    Some(start..end)
                } else {
                    None
                }
            }),
            "yesterday" => Self::now().and_then(|now| {
                let date = now.to_local().date() - Duration::days(1);
                let start = Self::try_from(date.and_hms(0, 0, 0)).map(|t| t.use_default_offset());
                let end =
                    Self::try_from(date.and_hms(23, 59, 59)).map(|t| t.use_default_offset() + 1);
                if let (Ok(start), Ok(Some(end))) = (start, end) {
                    Some(start..end)
                } else {
                    None
                }
            }),
            date if date.starts_with(">") => {
                Self::parse(&date[1..]).map(|start| start..Self::max_value())
            }
            date if date.starts_with("since ") => {
                Self::parse(&date[6..]).map(|start| start..Self::max_value())
            }
            date if date.starts_with("<") => {
                Self::parse(&date[1..]).map(|end| Self::min_value()..end)
            }
            date if date.starts_with("-") => {
                // This does not really make much sense. But is supported by hg
                // (see 'hg help dates').
                Self::parse_range(&format!("since {} days ago", &date[1..]))
            }
            date if date.starts_with("before ") => {
                Self::parse(&date[7..]).map(|end| Self::min_value()..end)
            }
            date if support_to && date.contains(" to ") => {
                let phrases: Vec<_> = date.split(" to ").collect();
                if phrases.len() == 2 {
                    if let (Some(start), Some(end)) = (
                        Self::parse_range_internal(&phrases[0], false),
                        Self::parse_range_internal(&phrases[1], false),
                    ) {
                        Some(start.start..end.end)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => {
                let start = Self::parse_absolute(date, default_date_lower);
                let end = Self::parse_absolute(date, default_date_upper::<N31>)
                    .or_else(|| Self::parse_absolute(date, default_date_upper::<N30>))
                    .or_else(|| Self::parse_absolute(date, default_date_upper::<N29>))
                    .or_else(|| Self::parse_absolute(date, default_date_upper::<N28>))
                    .and_then(|end| end + 1);
                if let (Some(start), Some(end)) = (start, end) {
                    Some(start..end)
                } else {
                    None
                }
            }
        }
    }

    /// Parse date in an absolute form.
    ///
    /// Return None if it cannot be parsed.
    ///
    /// `default_date` takes a format char, for example, `H`, and returns a
    /// default value of it.
    fn parse_absolute(date: &str, default_date: fn(char) -> &'static str) -> Option<Self> {
        let date = date.trim();

        // Hg internal format. "unixtime offset"
        let parts: Vec<_> = date.split(" ").collect();
        if parts.len() == 2 {
            if let Ok(unixtime) = parts[0].parse() {
                if let Ok(offset) = parts[1].parse() {
                    if is_valid_offset(offset) {
                        return Self { unixtime, offset }.bounded();
                    }
                }
            }
        }

        // Normalize UTC timezone name to +0000. The parser does not know
        // timezone names.
        let date = if date.ends_with("GMT") || date.ends_with("UTC") {
            format!("{} +0000", &date[..date.len() - 3])
        } else {
            date.to_string()
        };
        let mut now = None; // cached, lazily calculated "now"

        // Try all formats!
        for naive_format in DEFAULT_FORMATS.iter() {
            // Fill out default fields.  See mercurial.util.strdate.
            // This makes it possible to parse partial dates like "month/day",
            // or "hour:minute", since the missing fields will be filled.
            let mut default_format = String::new();
            let mut date_with_defaults = date.clone();
            let mut use_now = false;
            for part in ["S", "M", "HI", "d", "mb", "Yy"].iter() {
                if part
                    .chars()
                    .any(|ch| naive_format.contains(&format!("%{}", ch)))
                {
                    // For example, if the user specified "d" (day), but
                    // not other things, we should use 0 for "H:M:S", and
                    // "now" for "Y-m" (year, month).
                    use_now = true;
                } else {
                    let format_char = part.chars().nth(0).unwrap();
                    default_format += &format!(" @%{}", format_char);
                    if use_now {
                        // For example, if the user only specified "month/day",
                        // then we should use the current "year", instead of
                        // year 0.
                        now = now.or_else(|| Self::now().map(|n| n.to_local()));
                        match now {
                            Some(now) => {
                                date_with_defaults +=
                                    &format!(" @{}", now.format(&format!("%{}", format_char)))
                            }
                            None => return None,
                        }
                    } else {
                        // For example, if the user only specified
                        // "hour:minute", then we should use "second 0", instead
                        // of the current second.
                        date_with_defaults += " @";
                        date_with_defaults += default_date(format_char);
                    }
                }
            }

            // Try parse with timezone.
            // See https://docs.rs/chrono/0.4.9/chrono/format/strftime/index.html#specifiers
            let format = format!("{}%#z{}", naive_format, default_format);
            if let Ok(parsed) = DateTime::parse_from_str(&date_with_defaults, &format) {
                if let Ok(parsed) = parsed.try_into() {
                    return Some(parsed);
                }
            }

            // Without timezone.
            let format = format!("{}{}", naive_format, default_format);
            if let Ok(parsed) = NaiveDateTime::parse_from_str(&date_with_defaults, &format) {
                if let Ok(parsed) = parsed.try_into() {
                    return Some(parsed);
                }
            }
        }

        None
    }

    /// Change "offset" to DEFAULT_OFFSET. Useful for tests so they won't be
    /// affected by local timezone.
    fn use_default_offset(mut self) -> Self {
        let offset = DEFAULT_OFFSET.load(Ordering::SeqCst);
        if is_valid_offset(offset) {
            self.offset = offset
        }
        self
    }

    /// See [`HgTime::RANGE`] for details.
    pub const fn min_value() -> Self {
        Self {
            unixtime: -2208988800, // 1900-01-01 00:00:00
            offset: 0,
        }
    }

    /// See [`HgTime::RANGE`] for details.
    pub const fn max_value() -> Self {
        Self {
            unixtime: 253402300799, // 9999-12-31 23:59:59
            offset: 0,
        }
    }

    /// Return `None` if timestamp is out of [`HgTime::RANGE`].
    pub fn bounded(self) -> Option<Self> {
        if self < Self::min_value() || self > Self::max_value() {
            None
        } else {
            Some(self)
        }
    }
}

// Convert to compact u64.  Used by FORCED_NOW.
// For testing purpose only (no overflow checking).
impl HgTime {
    fn to_lossy_compact_u64(self) -> u64 {
        ((self.unixtime as u64) << 17) + (self.offset + 50401) as u64
    }

    fn from_compact_u64(value: u64) -> Self {
        let unixtime = (value as i64) >> 17;
        let offset = (((value & 0x1ffff) as i64) - 50401) as i32;
        Self { unixtime, offset }
    }
}

impl From<HgTime> for NaiveDateTime {
    fn from(time: HgTime) -> Self {
        time.to_naive()
    }
}

impl From<HgTime> for DateTime<Utc> {
    fn from(time: HgTime) -> Self {
        time.to_utc()
    }
}

impl From<HgTime> for DateTime<Local> {
    fn from(time: HgTime) -> Self {
        time.to_local()
    }
}

impl Add<u64> for HgTime {
    type Output = Option<Self>;

    fn add(self, seconds: u64) -> Option<Self> {
        seconds.try_into().ok().and_then(|seconds| {
            self.unixtime.checked_add(seconds).and_then(|unixtime| {
                Self {
                    unixtime,
                    offset: self.offset,
                }
                .bounded()
            })
        })
    }
}

impl Sub<u64> for HgTime {
    type Output = Option<Self>;

    fn sub(self, seconds: u64) -> Option<Self> {
        seconds.try_into().ok().and_then(|seconds| {
            self.unixtime.checked_sub(seconds).and_then(|unixtime| {
                Self {
                    unixtime,
                    offset: self.offset,
                }
                .bounded()
            })
        })
    }
}

impl PartialOrd for HgTime {
    fn partial_cmp(&self, other: &HgTime) -> Option<std::cmp::Ordering> {
        self.unixtime.partial_cmp(&other.unixtime)
    }
}

impl<Tz: TimeZone> TryFrom<DateTime<Tz>> for HgTime {
    type Error = ();
    fn try_from(time: DateTime<Tz>) -> Result<Self, ()> {
        if time.timestamp() >= i64::min_value() {
            Self {
                unixtime: time.timestamp(),
                offset: time.offset().fix().utc_minus_local(),
            }
            .bounded()
            .ok_or(())
        } else {
            Err(())
        }
    }
}

impl TryFrom<NaiveDateTime> for HgTime {
    type Error = ();
    fn try_from(time: NaiveDateTime) -> Result<Self, ()> {
        let offset = DEFAULT_OFFSET.load(Ordering::SeqCst);
        match FixedOffset::west_opt(offset) {
            Some(offset) => match offset.from_local_datetime(&time) {
                LocalResult::Single(datetime) => HgTime::try_from(datetime),
                _ => return Err(()),
            },
            None => match Local.from_local_datetime(&time) {
                LocalResult::Single(local) => HgTime::try_from(local),
                _ => return Err(()),
            },
        }
    }
}

/// Change default offset (timezone).
pub fn set_default_offset(offset: i32) {
    DEFAULT_OFFSET.store(offset, Ordering::SeqCst);
}

fn is_valid_offset(offset: i32) -> bool {
    // UTC-12 to UTC+14.
    offset >= -50400 && offset <= 43200
}

/// Lower bound for default values in dates.
fn default_date_lower(format_char: char) -> &'static str {
    match format_char {
        'H' | 'M' | 'S' => "00",
        'm' | 'd' => "1",
        _ => unreachable!(),
    }
}

trait ToStaticStr {
    fn to_static_str() -> &'static str;
}

struct N31;
struct N30;
struct N29;
struct N28;

impl ToStaticStr for N31 {
    fn to_static_str() -> &'static str {
        "31"
    }
}

impl ToStaticStr for N30 {
    fn to_static_str() -> &'static str {
        "30"
    }
}

impl ToStaticStr for N29 {
    fn to_static_str() -> &'static str {
        "29"
    }
}

impl ToStaticStr for N28 {
    fn to_static_str() -> &'static str {
        "28"
    }
}

/// Upper bound. Assume a month has `N::to_static_str()` days.
fn default_date_upper<N: ToStaticStr>(format_char: char) -> &'static str {
    match format_char {
        'H' => "23",
        'M' | 'S' => "59",
        'm' => "12",
        'd' => N::to_static_str(),
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_roundtrip() {
        let now = Local::now().with_nanosecond(0).unwrap();
        let hgtime: HgTime = now.try_into().unwrap();
        let now_again = DateTime::<Local>::from(hgtime);
        assert_eq!(now, now_again);
    }

    #[test]
    fn test_parse_date() {
        // Test cases are mostly from test-parse-date.t.
        // Some variants were added.
        set_default_offset(7200);

        // t: parse date
        // d: parse date, compare with now, within expected range
        // The right side of assert_eq! is a string so it's autofix-able.

        assert_eq!(t("2006-02-01 13:00:30"), t("2006-02-01 13:00:30-0200"));
        assert_eq!(t("2006-02-01 13:00:30-0500"), "1138816830 18000");
        assert_eq!(t("2006-02-01 13:00:30 +05:00"), "1138780830 -18000");
        assert_eq!(t("2006-02-01 13:00:30Z"), "1138798830 0");
        assert_eq!(t("2006-02-01 13:00:30 GMT"), "1138798830 0");
        assert_eq!(t("2006-4-5 13:30"), "1144251000 7200");
        assert_eq!(t("1150000000 14400"), "1150000000 14400");
        assert_eq!(t("100000 1400000"), "fail");
        assert_eq!(t("1000000000 -16200"), "1000000000 -16200");
        assert_eq!(t("2006-02-01 1:00:30PM +0000"), "1138798830 0");

        assert_eq!(d("1:00:30PM +0000", Duration::days(1)), "0");
        assert_eq!(d("02/01", Duration::weeks(52)), "0");
        assert_eq!(d("today", Duration::days(1)), "0");
        assert_eq!(d("yesterday", Duration::days(2)), "0");

        // ISO8601
        assert_eq!(t("2016-07-27T12:10:21"), "1469628621 7200");
        assert_eq!(t("2016-07-27T12:10:21Z"), "1469621421 0");
        assert_eq!(t("2016-07-27T12:10:21+00:00"), "1469621421 0");
        assert_eq!(t("2016-07-27T121021Z"), "1469621421 0");
        assert_eq!(t("2016-07-27 12:10:21"), "1469628621 7200");
        assert_eq!(t("2016-07-27 12:10:21Z"), "1469621421 0");
        assert_eq!(t("2016-07-27 12:10:21+00:00"), "1469621421 0");
        assert_eq!(t("2016-07-27 121021Z"), "1469621421 0");

        // Months
        assert_eq!(t("Jan 2018"), "1514772000 7200");
        assert_eq!(t("Feb 2018"), "1517450400 7200");
        assert_eq!(t("Mar 2018"), "1519869600 7200");
        assert_eq!(t("Apr 2018"), "1522548000 7200");
        assert_eq!(t("May 2018"), "1525140000 7200");
        assert_eq!(t("Jun 2018"), "1527818400 7200");
        assert_eq!(t("Jul 2018"), "1530410400 7200");
        assert_eq!(t("Sep 2018"), "1535767200 7200");
        assert_eq!(t("Oct 2018"), "1538359200 7200");
        assert_eq!(t("Nov 2018"), "1541037600 7200");
        assert_eq!(t("Dec 2018"), "1543629600 7200");
        assert_eq!(t("Foo 2018"), "fail");

        // Extra tests not in test-parse-date.t
        assert_eq!(d("Jan", Duration::weeks(52)), "0");
        assert_eq!(d("Jan 1", Duration::weeks(52)), "0"); // 1 is not considered as "year 1"
        assert_eq!(d("4-26", Duration::weeks(52)), "0");
        assert_eq!(d("4/26", Duration::weeks(52)), "0");
        assert_eq!(t("4/26/2000"), "956714400 7200");
        assert_eq!(t("Apr 26 2000"), "956714400 7200");
        assert_eq!(t("2020"), "1577844000 7200"); // 2020 is considered as a "year"
        assert_eq!(t("2020 GMT"), "1577836800 0");
        assert_eq!(t("2020-12"), "1606788000 7200");
        assert_eq!(t("2020-13"), "fail");
        assert_eq!(t("1000"), "fail"); // year 1000 < HgTime::min_value()
        assert_eq!(t("1"), "fail");
        assert_eq!(t("0"), "fail");
        assert_eq!(t("100000000000000000 1400"), "fail");

        assert_eq!(t("Fri, 20 Sep 2019 12:15:13 -0700"), "1569006913 25200"); // date --rfc-2822
        assert_eq!(t("Fri, 20 Sep 2019 12:15:13"), "1568988913 7200");
    }

    #[test]
    fn test_parse_ago() {
        set_default_offset(7200);
        assert_eq!(d("10m ago", Duration::hours(1)), "0");
        assert_eq!(d("10 min ago", Duration::hours(1)), "0");
        assert_eq!(d("10 minutes ago", Duration::hours(1)), "0");
        assert_eq!(d("10 hours ago", Duration::days(1)), "0");
        assert_eq!(d("10 h ago", Duration::days(1)), "0");
        assert_eq!(t("9999999 years ago"), "fail");
    }

    #[test]
    fn test_parse_range() {
        set_default_offset(7200);

        assert_eq!(c("since 1 month ago", "now"), "contains");
        assert_eq!(c("since 1 month ago", "2 months ago"), "does not contain");
        assert_eq!(c("> 1 month ago", "2 months ago"), "does not contain");
        assert_eq!(c("< 1 month ago", "2 months ago"), "contains");
        assert_eq!(c("< 1 month ago", "now"), "does not contain");

        assert_eq!(c("-3", "now"), "contains");
        assert_eq!(c("-3", "2 days ago"), "contains");
        assert_eq!(c("-3", "4 days ago"), "does not contain");

        assert_eq!(c("2018", "2017-12-31 23:59:59"), "does not contain");
        assert_eq!(c("2018", "2018-1-1"), "contains");
        assert_eq!(c("2018", "2018-12-31 23:59:59"), "contains");
        assert_eq!(c("2018", "2019-1-1"), "does not contain");

        assert_eq!(c("2018-5-1 to 2018-6-2", "2018-4-30"), "does not contain");
        assert_eq!(c("2018-5-1 to 2018-6-2", "2018-5-30"), "contains");
        assert_eq!(c("2018-5-1 to 2018-6-2", "2018-6-30"), "does not contain");
        assert_eq!(c("2018-5 to 2018-6", "2018-5-1 0:0:0"), "contains");
        assert_eq!(c("2018-5 to 2018-6", "2018-6-30 23:59:59"), "contains");
        assert_eq!(c("2018-5 to 2018-6 to 2018-7", "2018-6-30"), "fail");

        // 0:0:0 yesterday to 23:59:59 today
        // Usually it's 48 hours. However it might be affected by DST.
        let range = HgTime::parse_range("yesterday to today").unwrap();
        assert!(range.end.unixtime - range.start.unixtime >= (24 + 20) * 3600);
    }

    /// String representation of parse result.
    fn t(date: &str) -> String {
        match HgTime::parse(date) {
            Some(time) => format!("{} {}", time.unixtime, time.offset),
            None => "fail".to_string(),
        }
    }

    /// String representation of (parse result - now) / seconds.
    fn d(date: &str, duration: Duration) -> String {
        match HgTime::parse(date) {
            Some(time) => {
                let value = (time.unixtime as i64 - HgTime::now().unwrap().unixtime as i64).abs()
                    / duration.num_seconds();
                format!("{}", value)
            }
            None => "fail".to_string(),
        }
    }

    /// String "contains" (if range contains date) or "does not contain"
    /// or "fail" (if either range or date fails to parse).
    fn c(range: &str, date: &str) -> &'static str {
        if let (Some(range), Some(date)) = (HgTime::parse_range(range), HgTime::parse(date)) {
            if range.contains(&date) {
                "contains"
            } else {
                "does not contain"
            }
        } else {
            "fail"
        }
    }
}

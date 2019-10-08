// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! # parsedate
//!
//! See [`HgTime`] and [`HgTime::parse`] for main features.

use chrono::prelude::*;
use chrono::Duration;
use std::ops::{Add, Sub};
use std::sync::atomic::{AtomicI32, Ordering};

/// A simple time structure that matches hg's time representation.
///
/// Internally it's unixtime (in GMT), and offset (GMT -1 = +3600).
#[derive(Clone, Copy, Debug)]
pub struct HgTime {
    pub unixtime: u64,
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
static DEFAUL_OFFSET: AtomicI32 = AtomicI32::new(INVALID_OFFSET);

impl HgTime {
    pub fn now() -> Self {
        let now: HgTime = Local::now().into();
        now.use_default_offset()
    }

    /// Parse a date string.
    ///
    /// Return `None` if it cannot be parsed.
    ///
    /// This function matches `mercurial.util.parsedate`, and can parse
    /// some additional forms like `2 days ago`.
    pub fn parse(date: &str) -> Option<Self> {
        match date {
            "now" => Some(Self::now()),
            "today" => Some(Self::from(Local::today().and_hms(0, 0, 0)).use_default_offset()),
            "yesterday" => Some(
                Self::from(Local::today().and_hms(0, 0, 0) - Duration::days(1))
                    .use_default_offset(),
            ),
            _ => Self::parse_absolute(date),
        }
    }

    /// Parse date in an absolute form.
    ///
    /// Return None if it cannot be parsed.
    fn parse_absolute(date: &str) -> Option<Self> {
        let date = date.trim();

        // Hg internal format. "unixtime offset"
        let parts: Vec<_> = date.split(" ").collect();
        if parts.len() == 2 {
            if let Ok(unixtime) = parts[0].parse() {
                if let Ok(offset) = parts[1].parse() {
                    if is_valid_offset(offset) {
                        return Some(Self { unixtime, offset });
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
                        let now = now.get_or_insert_with(|| Local::now());
                        date_with_defaults +=
                            &format!(" @{}", now.format(&format!("%{}", format_char)));
                    } else {
                        // For example, if the user only specified
                        // "hour:minute", then we should use "second 0", instead
                        // of the current second.
                        match format_char {
                            'H' | 'M' | 'S' => date_with_defaults += " @00",
                            'Y' | 'm' | 'd' => date_with_defaults += " @1",
                            _ => unreachable!(),
                        }
                    }
                }
            }

            // Try parse with timezone.
            // See https://docs.rs/chrono/0.4.9/chrono/format/strftime/index.html#specifiers
            let format = format!("{}%#z{}", naive_format, default_format);
            if let Ok(parsed) = DateTime::parse_from_str(&date_with_defaults, &format) {
                return Some(parsed.into());
            }

            // Without timezone.
            let format = format!("{}{}", naive_format, default_format);
            if let Ok(parsed) = NaiveDateTime::parse_from_str(&date_with_defaults, &format) {
                return Some(parsed.into());
            }
        }

        None
    }

    /// Change "offset" to DEFAUL_OFFSET. Useful for tests so they won't be
    /// affected by local timezone.
    fn use_default_offset(mut self) -> Self {
        let offset = DEFAUL_OFFSET.load(Ordering::SeqCst);
        if is_valid_offset(offset) {
            self.offset = offset
        }
        self
    }
}

impl Add<u64> for HgTime {
    type Output = Self;

    fn add(self, seconds: u64) -> Self {
        Self {
            unixtime: self.unixtime + seconds,
            offset: self.offset,
        }
    }
}

impl Sub<u64> for HgTime {
    type Output = Self;

    fn sub(self, seconds: u64) -> Self {
        Self {
            // XXX: This might silently change negative time to 0.
            unixtime: self.unixtime.max(seconds) - seconds,
            offset: self.offset,
        }
    }
}

impl<Tz: TimeZone> From<DateTime<Tz>> for HgTime {
    fn from(time: DateTime<Tz>) -> Self {
        assert!(time.timestamp() >= 0);
        Self {
            unixtime: time.timestamp() as u64,
            offset: time.offset().fix().utc_minus_local(),
        }
    }
}

impl From<NaiveDateTime> for HgTime {
    fn from(time: NaiveDateTime) -> Self {
        let timestamp = time.timestamp();
        // Use local offset. (Is there a better way to do this?)
        let offset = Self::now().offset;
        // XXX: This might silently change negative time to 0.
        let unixtime = (timestamp + offset as i64).max(0) as u64;
        Self { unixtime, offset }
    }
}

/// Change default offset (timezone).
pub fn set_default_offset(offset: i32) {
    DEFAUL_OFFSET.store(offset, Ordering::SeqCst);
}

fn is_valid_offset(offset: i32) -> bool {
    // UTC-12 to UTC+14.
    offset >= -50400 && offset <= 43200
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_date() {
        // Test cases are mostly from test-parse-date.t.
        // Some variants were added.
        set_default_offset(7200);

        // t: parse date
        // d: parse date, compare with now, within expected range
        // The right side of assert_eq! is a string so it's autofix-able.

        assert_eq!(t("2006-02-01 13:00:30"), "1138806030 7200");
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

        assert_eq!(t("Fri, 20 Sep 2019 12:15:13 -0700"), "1569006913 25200"); // date --rfc-2822
        assert_eq!(t("Fri, 20 Sep 2019 12:15:13"), "1568988913 7200");
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
                let value = (time.unixtime as i64 - HgTime::now().unixtime as i64).abs()
                    / duration.num_seconds();
                format!("{}", value)
            }
            None => "fail".to_string(),
        }
    }
}

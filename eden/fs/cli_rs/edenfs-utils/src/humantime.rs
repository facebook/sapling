/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::time::Duration;

#[derive(PartialEq, PartialOrd)]
pub enum TimeUnit {
    Days,
    Hours,
    Minutes,
    Seconds,
    Milliseconds,
    Microseconds,
    Nanoseconds,
}

pub struct HumanTime {
    days: u16,
    hours: u16,
    minutes: u16,
    seconds: u16,
    milliseconds: u16,
    microseconds: u16,
    nanoseconds: u16,
}

impl HumanTime {
    /// Returns a string representing the human time like "1d" for 1 day
    /// with the highest time unit that has a value that is > 0
    /// up to the lowest_time_unit given.
    pub fn simple_human_time(&self, lowest_time_unit: TimeUnit) -> String {
        if self.days > 0 && lowest_time_unit >= TimeUnit::Days {
            format!("{}d", self.days)
        } else if self.hours > 0 && lowest_time_unit >= TimeUnit::Hours {
            format!("{}h", self.hours)
        } else if self.minutes > 0 && lowest_time_unit >= TimeUnit::Minutes {
            format!("{}m", self.minutes)
        } else if self.seconds > 0 && lowest_time_unit >= TimeUnit::Seconds {
            format!("{}s", self.seconds)
        } else if self.milliseconds > 0 && lowest_time_unit >= TimeUnit::Milliseconds {
            format!("{}ms", self.milliseconds)
        } else if self.microseconds > 0 && lowest_time_unit >= TimeUnit::Microseconds {
            format!("{}us", self.microseconds)
        } else if self.nanoseconds > 0 && lowest_time_unit >= TimeUnit::Nanoseconds {
            format!("{}ns", self.nanoseconds)
        } else {
            String::from(match lowest_time_unit {
                TimeUnit::Days => "0d",
                TimeUnit::Hours => "0h",
                TimeUnit::Minutes => "0m",
                TimeUnit::Seconds => "0s",
                TimeUnit::Milliseconds => "0ms",
                TimeUnit::Microseconds => "0\u{03BC}s",
                TimeUnit::Nanoseconds => "0ns",
            })
        }
    }
}

impl From<Duration> for HumanTime {
    fn from(duration: Duration) -> HumanTime {
        let ns_in_us = 1000;
        let ns_in_ms = 1000 * ns_in_us;
        let ns_in_seconds = 1000 * ns_in_ms;
        let ns_in_minutes = 60 * ns_in_seconds;
        let ns_in_hours = 60 * ns_in_minutes;
        let ns_in_days = 24 * ns_in_hours;

        let nanoseconds = duration.as_nanos();

        let days = nanoseconds / ns_in_days;
        let nanoseconds = nanoseconds % ns_in_days;

        let hours = nanoseconds / ns_in_hours;
        let nanoseconds = nanoseconds % ns_in_hours;

        let minutes = nanoseconds / ns_in_minutes;
        let nanoseconds = nanoseconds % ns_in_minutes;

        let seconds = nanoseconds / ns_in_seconds;
        let nanoseconds = nanoseconds % ns_in_seconds;

        let milliseconds = nanoseconds / ns_in_ms;
        let nanoseconds = nanoseconds % ns_in_ms;

        let microseconds = nanoseconds / ns_in_us;
        let nanoseconds = nanoseconds % ns_in_us;

        HumanTime {
            days: days.try_into().unwrap(),
            hours: hours.try_into().unwrap(),
            minutes: minutes.try_into().unwrap(),
            seconds: seconds.try_into().unwrap(),
            milliseconds: milliseconds.try_into().unwrap(),
            microseconds: microseconds.try_into().unwrap(),
            nanoseconds: nanoseconds.try_into().unwrap(),
        }
    }
}

impl fmt::Display for HumanTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            if self.days > 0 {
                write!(
                    f,
                    "{} day{}, ",
                    self.days,
                    if self.days == 1 { "" } else { "s" }
                )?;
            }
            write!(f, "{}:{:02}:{:02}", self.hours, self.minutes, self.seconds)
        } else {
            write!(
                f,
                "{}d:{:02}h:{:02}m:{:02}s",
                self.days, self.hours, self.minutes, self.seconds
            )
        }
    }
}

#[test]
fn test_simple_human_time_basic() {
    let test_duration = Duration::new(0, 300); // 300 ns
    assert_eq!(
        HumanTime::from(test_duration).simple_human_time(TimeUnit::Nanoseconds),
        "300ns",
    )
}

#[test]
fn test_simple_human_time_smaller_than_lowest_time_unit() {
    let test_duration = Duration::new(0, 300); // 300 ns
    assert_eq!(
        HumanTime::from(test_duration).simple_human_time(TimeUnit::Seconds),
        "0s",
    )
}

#[test]
fn test_simple_human_time_larger_than_lowest_time_unit() {
    let test_duration = Duration::new(5, 0); // 5 seconds
    assert_eq!(
        HumanTime::from(test_duration).simple_human_time(TimeUnit::Nanoseconds),
        "5s",
    )
}

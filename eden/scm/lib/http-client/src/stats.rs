/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::mem;
use std::time::Duration;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Stats {
    pub downloaded: usize,
    pub uploaded: usize,
    pub requests: usize,
    pub time: Duration,
    pub latency: Duration,
}

impl Stats {
    pub fn time_in_seconds(&self) -> f64 {
        self.time.as_secs_f64()
    }

    pub fn bytes_per_second(&self) -> f64 {
        self.downloaded as f64 / self.time_in_seconds()
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Downloaded {amount} in {time:.time_prec$?} over {requests} \
            request{plural} ({rate}, latency: {latency:.latency_prec$?})",
            amount = byte_count(self.downloaded),
            time = self.time,
            time_prec = if self.time.as_secs() == 0 { 0 } else { 2 },
            requests = self.requests,
            plural = if self.requests == 1 { "" } else { "s" },
            rate = bit_rate(self.bytes_per_second() * 8.0),
            latency = self.latency,
            latency_prec = if self.latency.as_secs() == 0 { 0 } else { 2 },
        )
    }
}

/// Format a byte count using SI binary prefixes.
fn byte_count(value: usize) -> String {
    if value == 0 {
        return "0 B".into();
    }

    // Compute the base-1024 log of the value (i.e., log2(value) / log2(1024)).
    let log = ((8 * mem::size_of::<usize>()) - value.leading_zeros() as usize - 1) / 10;

    // Shift value down. (Use floating-point division to preserve decimals.)
    let shifted = value as f64 / (1 << (log * 10)) as f64;

    // Determine unit and precision to display.
    let unit = ["B", "kiB", "MiB", "GiB", "TiB", "PiB", "EiB"][log];
    let prec = if log > 1 { 2 } else { 0 };

    format!("{:.*} {}", prec, shifted, unit)
}

/// Format a bit rate using decimal prefixes.
fn bit_rate(rate: f64) -> String {
    // Guard against zero, NaN, infinity, etc.
    if !rate.is_normal() {
        return "0 b/s".into();
    }

    // Divide by the base-1000 log of the value to bring it under 1000.
    let log = (rate.log10() / 3.0).floor() as usize;
    let shifted = rate / 1000f64.powi(log as i32);

    // Determine unit and precision to display.
    let unit = ["b/s", "kb/s", "Mb/s", "Gb/s", "Tb/s", "Pb/s", "Eb/s"][log];
    let prec = if log > 1 { 2 } else { 0 };

    format!("{:.*} {}", prec, shifted, unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stats_formatting() {
        let stats = Stats {
            downloaded: 10 * 1024 * 1024 + 600 * 1024, // 10.586 MiB
            uploaded: 1024,
            requests: 5,
            time: Duration::from_millis(12345),
            latency: Duration::from_micros(123456),
        };

        let expected = "Downloaded 10.59 MiB in 12.35s over 5 requests (7.19 Mb/s, latency: 123ms)";
        assert_eq!(expected, &stats.to_string());
    }
}

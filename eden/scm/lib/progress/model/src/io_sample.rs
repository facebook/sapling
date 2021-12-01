/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;
use std::time::Instant;

use once_cell::sync::Lazy;

static INSTANT_START: Lazy<Instant> = Lazy::new(Instant::now);

#[derive(Clone, Debug)]
pub struct IoSample {
    /// Total input bytes.
    input_bytes: u64,

    /// Total output bytes.
    output_bytes: u64,

    /// Count (ex. requests).
    count: u64,

    /// Timestamp the sample is taken.
    at: Instant,
}

/// A sample of total read / write bytes.
#[derive(Default)]
pub(crate) struct MutableIoSample {
    /// Total input bytes.
    input_bytes: AtomicU64,

    /// Total output bytes.
    output_bytes: AtomicU64,

    /// Count (ex. requests).
    count: AtomicU64,

    /// The sample is taken at this timestamp (milliseconds after UNIX_EPOCH).
    epoch: AtomicU64,
}

impl fmt::Debug for MutableIoSample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?}|{:?}|{:?}",
            self.input_bytes, self.output_bytes, self.count
        )
    }
}

impl IoSample {
    pub fn from_io_bytes(input_bytes: u64, output_bytes: u64) -> Self {
        Self::from_io_bytes_count(input_bytes, output_bytes, 0)
    }

    pub fn from_io_bytes_count(input_bytes: u64, output_bytes: u64, count: u64) -> Self {
        Self {
            input_bytes,
            output_bytes,
            count,
            at: Instant::now(),
        }
    }

    /// Set `at` for testing.
    pub(crate) fn test_at(mut self, millis: u64) -> Self {
        self.at = *INSTANT_START + Duration::from_millis(millis);
        self
    }
}

impl MutableIoSample {
    /// Update the sample to the specified data.
    pub(crate) fn set(&self, sample: IoSample) {
        self.input_bytes.store(sample.input_bytes, Relaxed);
        self.output_bytes.store(sample.output_bytes, Relaxed);
        self.count.store(sample.count, Relaxed);
        let millis = if sample.at <= *INSTANT_START {
            0
        } else {
            sample.at.duration_since(*INSTANT_START).as_millis()
        };
        self.epoch.store(millis as u64, Relaxed);
    }

    /// Sum of input and output bytes.
    pub(crate) fn total_bytes(&self) -> u64 {
        self.input_bytes.load(Relaxed) + self.output_bytes.load(Relaxed)
    }

    pub(crate) fn input_bytes(&self) -> u64 {
        self.input_bytes.load(Relaxed)
    }

    pub(crate) fn output_bytes(&self) -> u64 {
        self.output_bytes.load(Relaxed)
    }

    /// Number of (ex. requests).
    pub(crate) fn count(&self) -> u64 {
        self.count.load(Relaxed)
    }

    /// Milliseconds since `other`. Normalize to at least 1.
    fn millis_since(&self, other: &Self) -> u64 {
        self.epoch
            .load(Relaxed)
            .saturating_sub(other.epoch.load(Relaxed))
            .max(1)
    }

    /// Speed calculated from the previous state.
    /// Return (input_speed, output_speed), bytes per second.
    pub(crate) fn bytes_per_second(&self, prev: &Self) -> (u64, u64) {
        let input_delta = self
            .input_bytes
            .load(Relaxed)
            .saturating_sub(prev.input_bytes.load(Relaxed));
        let output_delta = self
            .output_bytes
            .load(Relaxed)
            .saturating_sub(prev.output_bytes.load(Relaxed));
        let millis_delta = self.millis_since(prev);
        (
            input_delta * 1000 / millis_delta,
            output_delta * 1000 / millis_delta,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl IoSample {
        pub(crate) fn to_mutable(self) -> MutableIoSample {
            let mutable_sample = MutableIoSample::default();
            mutable_sample.set(self);
            mutable_sample
        }
    }

    #[test]
    fn test_total_bytes() {
        let a = IoSample::from_io_bytes(10, 20).test_at(0).to_mutable();

        assert_eq!(a.total_bytes(), 30);
    }

    #[test]
    fn test_bytes_per_second() {
        let a = IoSample::from_io_bytes(10, 20).test_at(0).to_mutable();
        let b = IoSample::from_io_bytes(100, 200).test_at(2002).to_mutable();

        assert_eq!(b.millis_since(&a), 2002);
        assert_eq!(a.millis_since(&b), 1);

        assert_eq!(b.bytes_per_second(&a), (44, 89));
        assert_eq!(a.bytes_per_second(&b), (0, 0)); // no panic (saturating_sub)
        assert_eq!(a.bytes_per_second(&a), (0, 0)); // no panic
    }
}

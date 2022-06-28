/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use std::time::Duration;
use std::time::Instant;

#[derive(thiserror::Error, Debug)]
pub enum TimeseriesError {
    #[error("Conversion is out of bounds")]
    ConversionOutOfBounds(#[source] Error),
}

pub trait TimeseriesInstant<D>:
    std::ops::Sub<Self, Output = D>
    + std::ops::Add<D, Output = Self>
    + std::cmp::PartialOrd
    + std::cmp::Ord
    + Copy
    + Sized
{
}

pub trait TimeseriesDelta: Copy + Sized {
    fn div(&self, other: Self) -> Result<usize, Error>;
}

pub trait TimeseriesAccumulator: Default {
    type Value;

    fn insert(&mut self, value: Self::Value);
}

#[derive(Debug)]
pub struct Timeseries<A, D, I> {
    /// buckets represent [start, start + duration) intervals.
    buckets: Vec<A>,
    /// The start of this timeseries
    start_instant: I,
    /// The index in buckets corresponding to start_instant.
    start_idx: usize,
    /// How many buckets are valid, starting from the start bucket. If this is zero, then no
    /// buckets are valid at all.
    valid_count: usize,
    /// The duration of time that each bucket represents.
    interval: D,
}

impl<A, D, I> Timeseries<A, D, I>
where
    A: TimeseriesAccumulator,
    I: TimeseriesInstant<D>,
    D: TimeseriesDelta,
{
    pub fn new(start_instant: I, interval: D, buckets: usize) -> Self {
        let buckets = (0..buckets).map(|_| A::default()).collect();

        Self {
            start_instant,
            start_idx: 0,
            valid_count: 0,
            interval,
            buckets,
        }
    }

    /// Insert a new value at ts.
    pub fn insert(&mut self, ts: I, value: A::Value) -> Result<(), TimeseriesError> {
        if let Some(ref mut bucket) = self.bucket_for_ts(ts)? {
            bucket.insert(value);
        }

        Ok(())
    }

    /// Extend the time range to include ts, flagging all buckets before that interval as valid,
    /// such that they will be returned by [`Self::iter()`].
    pub fn update(&mut self, ts: I) -> Result<(), TimeseriesError> {
        self.bucket_for_ts(ts)?;

        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = &A> {
        self.buckets[self.start_idx..self.buckets.len()]
            .iter()
            .chain(self.buckets[0..self.start_idx].iter())
            .take(self.valid_count)
    }

    fn bucket_for_ts(&mut self, ts: I) -> Result<Option<&mut A>, TimeseriesError> {
        if ts < self.start_instant {
            return Ok(None);
        }

        let offset = (ts - self.start_instant)
            .div(self.interval)
            .map_err(TimeseriesError::ConversionOutOfBounds)?;

        let buckets_to_add = offset.saturating_sub(self.buckets.len() - 1);

        let offset = if buckets_to_add >= self.buckets.len() {
            self.reset_buckets(ts);
            0
        } else {
            self.add_buckets(buckets_to_add);
            offset - buckets_to_add
        };

        self.valid_count = std::cmp::max(offset + 1, self.valid_count);

        let pos = (self.start_idx + offset) % self.buckets.len();

        Ok(Some(&mut self.buckets[pos]))
    }

    fn reset_buckets(&mut self, ts: I) {
        for b in self.buckets.iter_mut() {
            *b = A::default();
        }

        self.start_instant = ts;
        self.start_idx = 0;
        self.valid_count = 0;
    }

    fn add_buckets(&mut self, n: usize) {
        let mut idx = self.start_idx;

        for _ in 0..n {
            let next_idx = (idx + 1) % self.buckets.len();
            self.buckets[idx] = A::default();
            self.start_idx = next_idx;
            self.valid_count = self.valid_count.saturating_sub(1);
            self.start_instant = self.start_instant + self.interval;
            idx = next_idx;
        }
    }
}

impl TimeseriesInstant<Duration> for Instant {}

impl TimeseriesDelta for Duration {
    fn div(&self, other: Self) -> Result<usize, Error> {
        let res = (self.as_micros() / other.as_micros()).try_into()?;
        Ok(res)
    }
}

impl TimeseriesInstant<usize> for usize {}

impl TimeseriesDelta for usize {
    fn div(&self, other: Self) -> Result<usize, Error> {
        Ok(self / other)
    }
}

impl TimeseriesInstant<u64> for u64 {}

impl TimeseriesDelta for u64 {
    fn div(&self, other: Self) -> Result<usize, Error> {
        (self / other).try_into().map_err(Error::from)
    }
}

impl<T> TimeseriesAccumulator for Vec<T> {
    type Value = T;

    fn insert(&mut self, value: Self::Value) {
        self.push(value);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_iter() -> Result<(), Error> {
        let mut ts = Timeseries::<Vec<u64>, _, _>::new(0usize, 1usize, 3);

        ts.insert(0, 0)?;
        ts.insert(1, 1)?;

        let d = ts.iter().cloned().collect::<Vec<_>>();
        assert_eq!(d, vec![vec![0], vec![1]]);

        ts.insert(3, 3)?;
        let d = ts.iter().cloned().collect::<Vec<_>>();
        assert_eq!(d, vec![vec![1], vec![], vec![3]]);

        Ok(())
    }

    #[test]
    fn test_basic() -> Result<(), Error> {
        let mut ts = Timeseries::<Vec<u64>, _, _>::new(0usize, 2usize, 2);
        assert_eq!(ts.start_instant, 0);
        assert_eq!(ts.start_idx, 0);
        assert_eq!(ts.buckets, vec![vec![], vec![]]);

        ts.insert(0, 1)?;
        assert_eq!(ts.start_instant, 0);
        assert_eq!(ts.start_idx, 0);
        assert_eq!(ts.buckets, vec![vec![1], vec![]]);

        ts.insert(1, 2)?;
        assert_eq!(ts.start_instant, 0);
        assert_eq!(ts.start_idx, 0);
        assert_eq!(ts.buckets, vec![vec![1, 2], vec![]]);

        ts.insert(2, 3)?;
        assert_eq!(ts.start_instant, 0);
        assert_eq!(ts.start_idx, 0);
        assert_eq!(ts.buckets, vec![vec![1, 2], vec![3]]);

        ts.insert(3, 4)?;
        assert_eq!(ts.start_instant, 0);
        assert_eq!(ts.start_idx, 0);
        assert_eq!(ts.buckets, vec![vec![1, 2], vec![3, 4]]);

        ts.insert(4, 5)?;
        assert_eq!(ts.start_instant, 2);
        assert_eq!(ts.start_idx, 1);
        assert_eq!(ts.buckets, vec![vec![5], vec![3, 4]]);

        Ok(())
    }

    #[test]
    fn test_add_buckets() -> Result<(), Error> {
        let mut ts = Timeseries::<Vec<u64>, _, _>::new(0usize, 2usize, 3);

        ts.insert(0, 1)?;
        ts.insert(2, 2)?;
        ts.insert(4, 3)?;
        let d = ts.iter().cloned().collect::<Vec<_>>();
        assert_eq!(d, vec![vec![1], vec![2], vec![3]]);

        ts.insert(8, 10)?;
        let d = ts.iter().cloned().collect::<Vec<_>>();
        assert_eq!(d, vec![vec![3], vec![], vec![10]]);

        Ok(())
    }

    #[test]
    fn test_reset_buckets() -> Result<(), Error> {
        let mut ts = Timeseries::<Vec<u64>, _, _>::new(0usize, 2usize, 3);

        ts.insert(0, 1)?;
        ts.insert(2, 2)?;
        ts.insert(4, 3)?;
        let d = ts.iter().cloned().collect::<Vec<_>>();
        assert_eq!(d, vec![vec![1], vec![2], vec![3]]);

        ts.insert(20, 20)?;
        let d = ts.iter().cloned().collect::<Vec<_>>();
        assert_eq!(d, vec![vec![20]]);

        ts.insert(22, 22)?;
        let d = ts.iter().cloned().collect::<Vec<_>>();
        assert_eq!(d, vec![vec![20], vec![22]]);

        ts.insert(30, 30)?;
        let d = ts.iter().cloned().collect::<Vec<_>>();
        assert_eq!(d, vec![vec![30]]);

        Ok(())
    }

    #[test]
    fn test_sequence() -> Result<(), Error> {
        #[derive(Default, Debug)]
        struct Last(usize);

        impl TimeseriesAccumulator for Last {
            type Value = usize;

            fn insert(&mut self, value: Self::Value) {
                self.0 = value;
            }
        }

        let mut ts = Timeseries::<Last, _, _>::new(0usize, 1usize, 15);

        for i in 0..30 {
            ts.insert(i, i)?;

            // Check that the timeseries always has x, x+1, x+2, etc.

            let mut last = None;

            for v in ts.iter() {
                let v = v.0;

                // Not initialized yet.
                if v == 0 {
                    continue;
                }

                if let Some(last) = last {
                    assert_eq!(v, last + 1);
                }
                last = Some(v);
            }

            assert_eq!(ts.iter().count(), std::cmp::min(i + 1, 15));

            // Check that it ends with i (which we just inserted). This will only be true stating
            // from the 14th element since we have 15 buckets so the last bucket is [14-15).

            if i >= 14 {
                assert_eq!(ts.iter().last().map(|v| v.0), Some(i));
            }
        }

        Ok(())
    }
}

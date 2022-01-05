/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::fmt;
use std::future::Future;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::AcqRel;
use std::sync::atomic::Ordering::Acquire;
use std::sync::atomic::Ordering::Release;
use std::sync::Arc;
use std::sync::Weak;
use std::time::Duration;

use crate::io_sample::MutableIoSample;
use crate::IoSample;

/// Time series for IO bytes. For example, Network [▁▂▄█▇▅▃▆] 3MB/s.
pub struct IoTimeSeries {
    /// Topic (ex. "Disk IO").
    topic: Cow<'static, str>,

    /// Unit of "count" (ex. "Requests").
    count_unit: Cow<'static, str>,

    /// Samples taken.
    samples: Arc<Vec<MutableIoSample>>,

    /// The "head" of the "deque" "samples".
    samples_head: Arc<AtomicUsize>,

    /// Maximum speed.
    max_bytes_per_second: AtomicU64,

    /// How to render time series.
    mode: TimeSeriesMode,
}

#[derive(Clone, Copy)]
pub enum TimeSeriesMode {
    /// Convert units to speed and show as human readable bytes/second.
    BytesSpeed,
    /// Display values as is, without any units and without converting to speed.
    ValueNoUnit,
}

impl IoTimeSeries {
    /// Create a time series showing bytes/sec speed.
    pub fn new(
        topic: impl Into<Cow<'static, str>>,
        count_unit: impl Into<Cow<'static, str>>,
    ) -> Arc<Self> {
        Self::new_with_mode(topic, count_unit, TimeSeriesMode::BytesSpeed)
    }

    /// Creates time series and customizes mode.
    pub fn new_with_mode(
        topic: impl Into<Cow<'static, str>>,
        count_unit: impl Into<Cow<'static, str>>,
        mode: TimeSeriesMode,
    ) -> Arc<Self> {
        let topic = topic.into();
        let count_unit = count_unit.into();
        let count = 16;
        let samples = (0..count).map(|_| Default::default()).collect();
        let series = Self {
            topic,
            count_unit,
            samples: Arc::new(samples),
            samples_head: Default::default(),
            max_bytes_per_second: Default::default(),
            mode,
        };
        Arc::new(series)
    }

    /// Default sampling interval.
    pub const fn default_sample_interval() -> Duration {
        Duration::from_secs(2)
    }

    /// Start sampling. This function can only be called once.
    /// The sampling future will end if the time series gets dropped.
    ///
    /// This function can only be called once per time series.
    /// Panic if called multiple times.
    pub fn async_sampling(
        &self,
        sample_func: impl Fn() -> IoSample + Send + Sync + 'static,
        interval: Duration,
    ) -> impl Future<Output = ()> {
        let len = self.samples.len();
        let samples = Arc::downgrade(&self.samples);
        let head = Arc::clone(&self.samples_head);
        let name = self.topic.clone();

        async move {
            tracing::debug!("start collecting samples for {}", name);
            loop {
                let sample = sample_func();
                if let Some(samples) = Weak::upgrade(&samples) {
                    let i = head.fetch_add(1, AcqRel);
                    samples[i % len].set(sample);
                } else {
                    // The "samples" has been dropped.
                    break;
                }
                tokio::time::sleep(interval).await;
            }
            tracing::debug!("stop collecting samples for {}", name);
        }
    }

    /// Add samples for test purpose.
    pub fn populate_test_samples(
        &self,
        input_scale: usize,
        output_scale: usize,
        count_scale: usize,
    ) {
        for (i, sample) in self.samples.iter().enumerate() {
            sample.set(
                IoSample::from_io_bytes_count(
                    (i * i * 5000 * input_scale) as _,
                    (i * i * 300 * output_scale) as _,
                    (i * count_scale) as _,
                )
                .test_at((i * 2000) as _),
            );
        }
        self.samples_head.store(self.samples.len() - 1, Release);
    }

    /// The topic of the time series.
    pub fn topic(&self) -> &str {
        &self.topic
    }

    /// The unit for "count".
    pub fn count_unit(&self) -> &str {
        &self.count_unit
    }

    /// Obtain the current speed. Bytes per second for (input, output).
    pub fn bytes_per_second(&self) -> (u64, u64) {
        let samples = &self.samples;
        let head = self.samples_head.load(Acquire);
        let len = samples.len();

        let cur = (head + len - 1) % len;
        let prev = (head + len - 2) % len;
        let old = &samples[prev];
        let new = &samples[cur];
        new.bytes_per_second(&old)
    }

    fn current_sample(&self) -> &MutableIoSample {
        let samples = &self.samples;
        let head = self.samples_head.load(Acquire);
        let len = samples.len();
        &samples[(head + len - 1) % len]
    }

    /// Obtains current total value.
    pub fn total_bytes(&self) -> u64 {
        self.current_sample().total_bytes()
    }

    /// Obtains current input bytes.
    pub fn input_bytes(&self) -> u64 {
        self.current_sample().input_bytes()
    }

    /// Obtains current output bytes.
    pub fn output_bytes(&self) -> u64 {
        self.current_sample().output_bytes()
    }

    /// Return speed units of this time series.
    pub fn mode(&self) -> TimeSeriesMode {
        self.mode
    }

    /// Get the count associated with the time series (from the last sample).
    pub fn count(&self) -> u64 {
        self.current_sample().count()
    }

    /// Are the samples empty (no progress at all)?
    pub fn is_stale(&self) -> bool {
        let min = self.samples.iter().map(|s| s.total_bytes()).min();
        let max = self.samples.iter().map(|s| s.total_bytes()).max();
        min == max
    }

    /// Render the time series into an array with max value being `max`.
    pub fn scaled_speeds(&self, max: u8) -> Vec<u8> {
        let max = max.max(1);
        let samples = &self.samples;
        let head = self.samples_head.load(Acquire);
        let len = samples.len();
        let bytes_per_second_list: Vec<u64> = {
            (1..len)
                .map(|i| {
                    let prev = (head + i - 1) % len;
                    let cur = (prev + 1) % len;
                    match self.mode {
                        TimeSeriesMode::BytesSpeed => {
                            let (i, o) = samples[cur].bytes_per_second(&samples[prev]);
                            i + o
                        }
                        TimeSeriesMode::ValueNoUnit => samples[cur].total_bytes(),
                    }
                })
                .collect()
        };

        let bytes_per_second_max = bytes_per_second_list
            .iter()
            .cloned()
            .max()
            .unwrap_or(1)
            .max(1);
        let bytes_per_second_max = bytes_per_second_max.max(
            self.max_bytes_per_second
                .fetch_max(bytes_per_second_max, AcqRel),
        );

        let values: Vec<u8> = bytes_per_second_list
            .into_iter()
            .map(|bytes_per_second| {
                if bytes_per_second == 0 || bytes_per_second_max <= 1 {
                    0u8
                } else {
                    (1 + ((bytes_per_second - 1) * (max - 1) as u64) / (bytes_per_second_max - 1))
                        as u8
                }
            })
            .collect();
        values.into_iter().collect()
    }
}

impl fmt::Debug for IoTimeSeries {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {:?}", &self.topic, &self.samples)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering::AcqRel;
    use std::sync::atomic::Ordering::Acquire;

    use super::*;
    const GAUGE_CHARS: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    #[tokio::test]
    async fn test_bytes_per_second() {
        let count = Arc::new(AtomicU64::new(0));
        let count2 = count.clone();
        let take_sample = move || -> IoSample {
            let i = count.fetch_add(1, AcqRel);
            IoSample::from_io_bytes(i * 1000, i * 20).test_at(i)
        };

        let series = IoTimeSeries::new("IO", "files");
        let interval = Duration::from_millis(10);
        let sampling_task = tokio::task::spawn(series.async_sampling(take_sample, interval));

        // Wait until `take_sample` is called a few times.
        while count2.load(Acquire) < 3 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(series.bytes_per_second(), (1000000, 20000));
        drop(series);

        // The sampling task exits when the main series gets dropped.
        sampling_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_ascii_graph() {
        let count = Arc::new(AtomicU64::new(0));
        let last = Arc::new(AtomicU64::new(0));
        let count2 = count.clone();
        const N: u64 = GAUGE_CHARS.len() as u64;
        let take_sample = move || -> IoSample {
            // Generate ▁▂▃▄▅▆▇█ graph.
            let i = count.fetch_add(1, AcqRel);
            let ii = i % N;
            let v = last.fetch_add(ii, AcqRel);
            IoSample::from_io_bytes(v, 0).test_at(i)
        };

        let series = IoTimeSeries::new("IO", "files");
        let interval = Duration::from_millis(10);
        let sampling_task = tokio::task::spawn(series.async_sampling(take_sample, interval));

        let sample_count = series.samples.len() as u64;
        loop {
            let count = count2.load(Acquire);
            let ascii = series.ascii_graph();
            assert_eq!(ascii.chars().count() as u64, sample_count - 1);
            tokio::time::sleep(Duration::from_millis(10)).await;
            if count <= sample_count + 1 {
                // Wait for enough samples.
                continue;
            }
            assert!(
                "▁▂▃▄▅▆▇█ ▁▂▃▄▅▆▇█ ▁▂▃▄▅▆▇█".contains(ascii.trim()),
                "unexpected graph: {:?}",
                &ascii
            );
            if count > sample_count * 3 + 1 {
                // Tested enough.
                break;
            }
        }

        drop(series);
        sampling_task.await.unwrap();
    }

    impl IoTimeSeries {
        fn ascii_graph(&self) -> String {
            let v = self.scaled_speeds((GAUGE_CHARS.len() - 1) as u8);
            v.into_iter().map(|i| GAUGE_CHARS[i as usize]).collect()
        }
    }
}

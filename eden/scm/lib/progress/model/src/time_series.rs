/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::io_sample::MutableIoSample;
use crate::IoSample;
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
}

impl IoTimeSeries {
    /// Create a time series.
    pub fn new(
        topic: impl Into<Cow<'static, str>>,
        count_unit: impl Into<Cow<'static, str>>,
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
}

impl fmt::Debug for IoTimeSeries {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {:?}", &self.topic, &self.samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering::AcqRel;
    use std::sync::atomic::Ordering::Acquire;
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
}

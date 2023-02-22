/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::VecDeque;
use std::fmt::Display;
use std::hash::Hash;
use std::time::Duration;
use std::time::Instant;

use dashmap::DashMap;

struct WeightSlidingWindow {
    start: Instant,
    window: Duration,
    // Always monotonically increasing times
    entries: VecDeque<(Instant, f64)>,
    // Sum of the weights of entries
    total_weight: f64,
}

impl WeightSlidingWindow {
    fn start_at(start: Instant, window: Duration) -> Self {
        Self {
            start,
            window,
            entries: VecDeque::new(),
            total_weight: 0.,
        }
    }

    fn prune(&mut self) -> Instant {
        let now = Instant::now();
        while let Some((last, weight)) = self.entries.front() {
            if *last
                < now.checked_sub(self.window).unwrap_or_else(|| {
                    panic!(
                        "Duration now ({:?}) is less than window ({:?})",
                        now, self.window
                    )
                })
            {
                self.total_weight -= weight;
                self.entries.pop_front();
            } else {
                break;
            }
        }
        now
    }

    /// Average weight over the window, if any
    pub fn avg(&mut self) -> Option<f64> {
        self.prune();
        if self.entries.is_empty() {
            None
        } else {
            Some(self.total_weight / (self.entries.len() as f64))
        }
    }

    /// How much weight is consumed per second
    /// If weight is number of operations, then this means speed.
    /// If weight is seconds doing something, then this is (approximately)
    /// saturation (less accurate with longer weights or shorter windows).
    pub fn wps(&mut self) -> f64 {
        let now = self.prune();
        self.total_weight / ((now - self.start).min(self.window).as_secs_f64())
    }

    pub fn add_entry(&mut self, weight: f64) -> Instant {
        let now = self.prune();
        self.total_weight += weight;
        self.entries.push_back((now, weight));
        now
    }
}

pub struct AvgTimeSlidingWindows<Key> {
    start: Instant,
    window: Duration,
    sliding_windows: DashMap<Key, WeightSlidingWindow>,
}

impl<Key: Eq + Hash> AvgTimeSlidingWindows<Key> {
    pub fn start(window: Duration) -> Self {
        Self {
            start: Instant::now(),
            window,
            sliding_windows: DashMap::new(),
        }
    }

    pub fn add_entry(&self, key: Key, duration: Duration) -> Instant {
        self.sliding_windows
            .entry(key)
            .or_insert_with(|| WeightSlidingWindow::start_at(self.start, self.window))
            .add_entry(duration.as_secs_f64())
    }
}

impl<Key: Eq + Hash + Display> Display for AvgTimeSlidingWindows<Key> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut first = true;
        for mut r in self.sliding_windows.iter_mut() {
            if !first {
                write!(f, ", ")?;
            }
            first = false;
            write!(f, "{} avg ", r.key())?;
            if let Some(avg) = r.value_mut().avg() {
                write!(f, "{:.2?}", Duration::from_secs_f64(avg))?;
            } else {
                write!(f, "<NO ENTRIES>")?;
            }
        }
        Ok(())
    }
}

pub struct BasicSpeedTracker {
    s_10m: WeightSlidingWindow,
    s_1h: WeightSlidingWindow,
    s_1d: WeightSlidingWindow,
}

impl BasicSpeedTracker {
    pub fn start() -> Self {
        let now = Instant::now();
        Self {
            s_10m: WeightSlidingWindow::start_at(now, Duration::from_secs(60 * 10)),
            s_1h: WeightSlidingWindow::start_at(now, Duration::from_secs(60 * 60)),
            s_1d: WeightSlidingWindow::start_at(now, Duration::from_secs(60 * 60 * 24)),
        }
    }

    #[allow(dead_code)]
    pub fn add_entry(&mut self) {
        self.add_entries(1)
    }

    pub fn add_entries(&mut self, count: usize) {
        let count = count as f64;
        let Self { s_10m, s_1h, s_1d } = self;
        s_10m.add_entry(count);
        s_1h.add_entry(count);
        s_1d.add_entry(count);
    }

    pub fn human_readable(&mut self) -> String {
        let Self { s_10m, s_1h, s_1d } = self;

        format!(
            "Sliding window speeds: {:.2}/s (10m) {:.2}/s (1h) {:.2}/s (1d)",
            s_10m.wps(),
            s_1h.wps(),
            s_1d.wps()
        )
    }
}

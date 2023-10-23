/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::fmt;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::AcqRel;
use std::sync::atomic::Ordering::Acquire;
use std::sync::atomic::Ordering::Release;
use std::sync::Arc;
use std::sync::Weak;
use std::time::Duration;
use std::time::Instant;

use arc_swap::ArcSwapOption;
use parking_lot::Mutex;

use crate::Registry;

/// A progress bar. It has multiple `Metric`s and a `Metric`.
///
/// ```plain,ignore
/// topic [ message ] [ pos / total unit1 ], [ pos / total unit2 ], ...
/// ```
pub struct ProgressBar {
    topic: Cow<'static, str>,
    message: ArcSwapOption<String>,
    pos: AtomicU64,
    total: AtomicU64,
    unit: Cow<'static, str>,
    created_at: Instant,
    started_at: ArcSwapOption<Instant>,
    finished_at: ArcSwapOption<Instant>,
}

struct Builder {
    registry: Registry,
    register: bool,
    topic: Cow<'static, str>,
    total: u64,
    unit: Cow<'static, str>,
}

impl Builder {
    fn new() -> Self {
        Builder {
            registry: Registry::main().clone(),
            register: true,
            topic: "".into(),
            total: 0,
            unit: "".into(),
        }
    }

    fn registry(mut self, r: &Registry) -> Self {
        self.registry = r.clone();
        self
    }

    fn topic(mut self, t: impl Into<Cow<'static, str>>) -> Self {
        self.topic = t.into();
        self
    }

    fn total(mut self, t: u64) -> Self {
        self.total = t;
        self
    }

    fn unit(mut self, u: impl Into<Cow<'static, str>>) -> Self {
        self.unit = u.into();
        self
    }

    fn register(mut self, r: bool) -> Self {
        self.register = r;
        self
    }

    fn started(self) -> Arc<ProgressBar> {
        let bar = self.pending();
        bar.start();
        bar
    }

    fn pending(self) -> Arc<ProgressBar> {
        let bar = Arc::new(ProgressBar {
            topic: self.topic,
            unit: self.unit,
            total: AtomicU64::new(self.total),
            pos: Default::default(),
            message: Default::default(),
            created_at: Instant::now(),
            started_at: Default::default(),
            finished_at: Default::default(),
        });
        if self.register {
            self.registry.register_progress_bar(&bar);
        }
        bar
    }
}

#[derive(Debug, PartialEq)]
pub enum BarState {
    Pending,
    Running,
    Complete,
}

impl ProgressBar {
    /// Create a new progress bar of the given topic (ex. "writing files").
    pub fn new(
        topic: impl Into<Cow<'static, str>>,
        total: u64,
        unit: impl Into<Cow<'static, str>>,
    ) -> Arc<Self> {
        Builder::new()
            .topic(topic)
            .total(total)
            .unit(unit)
            .register(false)
            .pending()
    }

    /// Create a new progress bar and register with default registry.
    pub fn register_new(
        topic: impl Into<Cow<'static, str>>,
        total: u64,
        unit: impl Into<Cow<'static, str>>,
    ) -> Arc<Self> {
        Builder::new()
            .topic(topic)
            .total(total)
            .unit(unit)
            .started()
    }

    fn start(&self) {
        self.started_at.store(Some(Arc::new(Instant::now())));
    }

    fn finish(&self) {
        self.finished_at.store(Some(Arc::new(Instant::now())));
    }

    /// Get the progress bar topic.
    pub fn topic(&self) -> &str {
        &self.topic
    }

    /// Get the progress message.
    pub fn message(&self) -> Option<Arc<String>> {
        self.message.load_full()
    }

    /// Set the progress message.
    pub fn set_message(&self, message: String) {
        self.message.store(Some(Arc::new(message)));
    }

    /// Obtain the position and total.
    pub fn position_total(&self) -> (u64, u64) {
        let pos = self.pos.load(Acquire);
        let total = self.total.load(Acquire);
        (pos, total)
    }

    /// Set total.
    pub fn set_total(&self, total: u64) {
        self.total.store(total, Release);
    }

    /// Set position.
    pub fn set_position(&self, pos: u64) {
        self.pos.store(pos, Release);
    }

    /// Increase position.
    pub fn increase_position(&self, inc: u64) {
        self.pos.fetch_add(inc, AcqRel);
    }

    /// Increase total.
    pub fn increase_total(&self, inc: u64) {
        self.total.fetch_add(inc, AcqRel);
    }

    /// Obtain unit.
    pub fn unit(&self) -> &str {
        &self.unit
    }

    /// Time since the creation of the progress bar.
    pub fn since_creation(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Time since the progress bar started, up to `finished_at` if finished,
    /// else now.
    pub fn since_start(&self) -> Option<Duration> {
        let started_at = self.started_at.load();
        let started_at = started_at.as_ref()?;
        if let Some(finished_at) = self.finished_at.load().as_ref() {
            Some(finished_at.duration_since(**started_at))
        } else {
            Some(started_at.elapsed())
        }
    }

    pub fn state(&self) -> BarState {
        if self.started_at.load().is_none() {
            BarState::Pending
        } else if self.finished_at.load().is_none() {
            BarState::Running
        } else {
            BarState::Complete
        }
    }
}

impl fmt::Debug for ProgressBar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (pos, total) = self.position_total();
        write!(f, "[{} {}/{} {}", self.topic(), pos, total, self.unit())?;
        if let Some(message) = self.message() {
            write!(f, " {}", message)?;
        }
        Ok(())
    }
}

pub struct AggregatingProgressBar {
    bar: Mutex<Weak<ProgressBar>>,
    topic: Cow<'static, str>,
    unit: Cow<'static, str>,
}

/// AggregatingProgressBar allows sharing a progress bar across
/// concurrent uses when otherwise inconvenient. For example, it lets
/// you display a single progress bar via a low level client object
/// when that client is used by multiple high level threads.
impl AggregatingProgressBar {
    pub fn new(
        topic: impl Into<Cow<'static, str>>,
        unit: impl Into<Cow<'static, str>>,
    ) -> Arc<Self> {
        Arc::new(AggregatingProgressBar {
            bar: Mutex::new(Weak::new()),
            topic: topic.into(),
            unit: unit.into(),
        })
    }

    /// If progress bar exists, increase its total, otherwise create a
    /// new progress bar. You should avoid calling set_position or
    /// set_total on the returned ProgressBar.
    pub fn create_or_extend(&self, additional_total: u64) -> Arc<ProgressBar> {
        let mut bar = self.bar.lock();

        match bar.upgrade() {
            Some(bar) => {
                bar.increase_total(additional_total);
                bar
            }
            None => {
                let new_bar = ProgressBar::register_new(
                    self.topic.clone(),
                    additional_total,
                    self.unit.clone(),
                );
                *bar = Arc::downgrade(&new_bar);
                new_bar
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregating_bar() {
        let agg = AggregatingProgressBar::new("eat", "apples");

        {
            let bar1 = agg.create_or_extend(10);
            bar1.increase_position(5);
            assert_eq!((5, 10), agg.create_or_extend(0).position_total());

            {
                let bar2 = agg.create_or_extend(5);
                bar2.increase_position(5);
                assert_eq!((10, 15), agg.create_or_extend(0).position_total());
            }

            assert_eq!((10, 15), agg.create_or_extend(0).position_total());
        }

        Registry::main().remove_orphan_progress_bar();

        assert_eq!((0, 0), agg.create_or_extend(0).position_total());
    }

    #[test]
    fn test_elapsed() {
        let bar = ProgressBar::new("", 0, "");

        assert_eq!(bar.state(), BarState::Pending);
        assert_eq!(bar.since_start(), None);

        bar.start();

        assert_eq!(bar.state(), BarState::Running);

        let elapsed_running = bar.since_start().unwrap();

        bar.finish();

        assert_eq!(bar.state(), BarState::Complete);

        let elapsed_complete = bar.since_start().unwrap();
        // Elapsed advanced further as we were running.
        assert!(elapsed_complete > elapsed_running);
        // But doesn't advance any further now that we are complete.
        assert_eq!(elapsed_complete, bar.since_start().unwrap());
    }

    #[test]
    fn test_builder() {
        let reg = Registry::default();

        let bar = Builder::new()
            .topic("hello")
            // We can override registry.
            .registry(&reg)
            .started();
        assert_eq!(reg.list_progress_bar().len(), 1);
        assert!(bar.since_start().is_some());
        assert_eq!(bar.topic(), "hello");
    }
}

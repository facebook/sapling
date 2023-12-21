/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::atomic::Ordering::AcqRel;
use std::sync::atomic::Ordering::Acquire;
use std::sync::atomic::Ordering::Release;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::Weak;
use std::time::Duration;
use std::time::Instant;

use arc_swap::ArcSwapOption;
use parking_lot::Mutex;

use crate::Registry;

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A progress bar. It has multiple `Metric`s and a `Metric`.
///
/// ```plain,ignore
/// topic [ message ] [ pos / total unit1 ], [ pos / total unit2 ], ...
/// ```
pub struct ProgressBar {
    id: u64,
    topic: Cow<'static, str>,
    message: ArcSwapOption<String>,
    pos: AtomicU64,
    total: AtomicU64,
    unit: Cow<'static, str>,
    created_at: Instant,
    started_at: OnceLock<Instant>,
    finished_at: OnceLock<Instant>,

    // Note that this is a strong reference, which could slow down orphaned bar
    // cleanup. In practice we probably could use a weak reference here, but if
    // we do "lose" a child progress bar to another thread, it would be useful
    // to see the child's ancestor bars (even if they have gone out of scope).
    parent: Option<Arc<ProgressBar>>,

    // If `true`, bar is transient (i.e. expected to show up as it
    // starts and disappear when it finishes). If `false`, bar is is
    // intended to be a planned "phase" that is displayed to the user
    // before it starts and after it finishes. Only impacts rendering.
    adhoc: bool,
}

pub struct Builder {
    registry: Registry,
    register: bool,
    topic: Cow<'static, str>,
    total: u64,
    unit: Cow<'static, str>,
    parent: Option<Arc<ProgressBar>>,
    adhoc: bool,
}

impl Builder {
    pub fn new() -> Self {
        Builder {
            registry: Registry::main().clone(),
            register: true,
            topic: "".into(),
            total: 0,
            unit: "".into(),
            parent: None,
            adhoc: true,
        }
    }

    pub fn registry(mut self, r: &Registry) -> Self {
        self.registry = r.clone();
        self
    }

    pub fn topic(mut self, t: impl Into<Cow<'static, str>>) -> Self {
        self.topic = t.into();
        self
    }

    pub fn total(mut self, t: u64) -> Self {
        self.total = t;
        self
    }

    pub fn unit(mut self, u: impl Into<Cow<'static, str>>) -> Self {
        self.unit = u.into();
        self
    }

    pub fn register(mut self, r: bool) -> Self {
        self.register = r;
        self
    }

    pub fn thread_local_parent(mut self) -> Self {
        self.parent = self.registry.get_active_progress_bar();
        self
    }

    pub fn adhoc(mut self, a: bool) -> Self {
        self.adhoc = a;
        self
    }

    pub fn active(self) -> ActiveProgressBar {
        let registry = self.registry.clone();
        let bar = self.thread_local_parent().register(true).pending();
        ProgressBar::push_active(bar, &registry)
    }

    pub fn pending(self) -> Arc<ProgressBar> {
        let bar = Arc::new(ProgressBar {
            id: ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            topic: self.topic,
            unit: self.unit,
            total: AtomicU64::new(self.total),
            pos: Default::default(),
            message: Default::default(),
            created_at: Instant::now(),
            started_at: Default::default(),
            finished_at: Default::default(),
            parent: self.parent,
            adhoc: self.adhoc,
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
    /// Will not be displayed until you register it.
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

    /// Create, register, and start a progress bar as a child of the thread's
    /// active progress bar. Bar will get cleaned up when it goes out of scope.
    /// Most places should use this.
    pub fn new_adhoc(
        topic: impl Into<Cow<'static, str>>,
        total: u64,
        unit: impl Into<Cow<'static, str>>,
    ) -> ActiveProgressBar {
        Builder::new().topic(topic).total(total).unit(unit).active()
    }

    /// Create and register a progress bar as a child of the thread's active
    /// progress bar, but don't start it. Call push_active to start it. This is
    /// useful to pre-register a list of phases which will show up before/after
    /// they've started/finished.
    pub fn new_pending(
        topic: impl Into<Cow<'static, str>>,
        total: u64,
        unit: impl Into<Cow<'static, str>>,
    ) -> Arc<Self> {
        Builder::new()
            .topic(topic)
            .total(total)
            .unit(unit)
            .thread_local_parent()
            .adhoc(false)
            .pending()
    }

    /// Create and register a progress bar that displays as a detached
    /// progress bar (i.e. not a child of another bar). Bar is not
    /// started, so will not display elapsed time.
    pub fn new_detached(
        topic: impl Into<Cow<'static, str>>,
        total: u64,
        unit: impl Into<Cow<'static, str>>,
    ) -> Arc<Self> {
        Builder::new()
            .topic(topic)
            .total(total)
            .unit(unit)
            .pending()
    }

    /// Start `bar` and set as active. When returned guard is dropped, progress
    /// bar will be marked finished and unset as the active bar.
    pub fn push_active(bar: Arc<Self>, registry: &Registry) -> ActiveProgressBar {
        Self::set_active(&bar, registry);
        ActiveProgressBar {
            bar,
            registry: registry.clone(),
            _phantom: PhantomData,
        }
    }

    /// Mark `bar` as finished and unset it as the active progress bar. This is
    /// exposed for Python use - you probably don't want to call it directly.
    pub fn pop_active(bar: &Arc<Self>, registry: &Registry) {
        bar.finish();

        if let Some(active) = registry.get_active_progress_bar() {
            // Only update things if we are the active bar.
            if Arc::ptr_eq(&active, bar) {
                let mut parent = bar.parent.as_ref();

                // Bars could have been dropped out of order. Set our first
                // non-finished ancestor as the active bar.
                while let Some(bar) = parent {
                    if bar.state() != BarState::Complete {
                        break;
                    }
                    parent = bar.parent.as_ref();
                }

                registry.set_active_progress_bar(parent.cloned());
            }
        }
    }

    /// Start `bar` and set as active. It is up to the caller to call
    /// pop_active. This is exposed for Python use - you probably don't want to
    /// call it directly.
    pub fn set_active(bar: &Arc<Self>, registry: &Registry) {
        bar.start();
        registry.set_active_progress_bar(Some(bar.clone()));
    }

    fn start(&self) {
        let _ = self.started_at.set(Instant::now());
    }

    fn finish(&self) {
        let _ = self.finished_at.set(Instant::now());
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
        let started_at = self.started_at.get()?;
        if let Some(finished_at) = self.finished_at.get() {
            Some(finished_at.duration_since(*started_at))
        } else {
            Some(started_at.elapsed())
        }
    }

    pub fn state(&self) -> BarState {
        if self.started_at.get().is_none() {
            BarState::Pending
        } else if self.finished_at.get().is_none() {
            BarState::Running
        } else {
            BarState::Complete
        }
    }

    pub fn parent(&self) -> Option<Arc<Self>> {
        self.parent.clone()
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn adhoc(&self) -> bool {
        self.adhoc
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

impl std::ops::Deref for ActiveProgressBar {
    type Target = Arc<ProgressBar>;

    fn deref(&self) -> &Self::Target {
        &self.bar
    }
}

pub struct ActiveProgressBar {
    bar: Arc<ProgressBar>,
    registry: Registry,
    // Disallow Sending to other threads.
    _phantom: PhantomData<Rc<()>>,
}

impl Drop for ActiveProgressBar {
    fn drop(&mut self) {
        ProgressBar::pop_active(&self.bar, &self.registry);
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
                let new_bar = ProgressBar::new_detached(
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
            .pending();
        assert_eq!(reg.list_progress_bar().len(), 1);
        assert!(bar.since_start().is_none());
        assert_eq!(bar.topic(), "hello");
    }

    #[test]
    fn test_active_bar_per_thread() {
        let reg = Registry::default();

        assert!(reg.get_active_progress_bar().is_none());

        let bar = Builder::new().registry(&reg).active();
        assert!(Arc::ptr_eq(&reg.get_active_progress_bar().unwrap(), &*bar));

        let reg2 = reg.clone();
        std::thread::spawn(|| {
            let reg = reg2;

            assert!(reg.get_active_progress_bar().is_none());

            {
                let bar = Builder::new().registry(&reg).active();
                assert!(Arc::ptr_eq(&reg.get_active_progress_bar().unwrap(), &*bar));
            }

            assert!(reg.get_active_progress_bar().is_none());
        })
        .join()
        .unwrap();

        assert!(Arc::ptr_eq(&reg.get_active_progress_bar().unwrap(), &*bar));

        drop(bar);

        assert!(reg.get_active_progress_bar().is_none());
    }

    #[test]
    fn test_active_bar_nested() {
        let reg = Registry::default();

        let bar1 = Builder::new().registry(&reg).active();
        assert!(Arc::ptr_eq(&reg.get_active_progress_bar().unwrap(), &*bar1));

        let bar2 = Builder::new().registry(&reg).active();
        assert!(Arc::ptr_eq(&reg.get_active_progress_bar().unwrap(), &*bar2));

        drop(bar2);
        assert!(Arc::ptr_eq(&reg.get_active_progress_bar().unwrap(), &*bar1));

        drop(bar1);
        assert!(&reg.get_active_progress_bar().is_none());
    }

    #[test]
    fn test_active_bar_dont_leak() {
        let reg = Registry::default();

        let bar = Builder::new().registry(&reg).pending();
        assert!(reg.get_active_progress_bar().is_none());

        ProgressBar::set_active(&bar, &reg);
        assert!(Arc::ptr_eq(&reg.get_active_progress_bar().unwrap(), &bar));

        // Didn't pop_active for whatever reason.
        drop(bar);

        // We are still active.
        assert!(reg.get_active_progress_bar().is_some());

        // But eventually we will get cleaned up.
        reg.remove_orphan_models();
        assert!(reg.get_active_progress_bar().is_none());
        assert!(reg.list_progress_bar().is_empty());
    }

    #[test]
    fn test_active_bar_manual_management() {
        let reg = Registry::default();

        let bar1 = Builder::new().registry(&reg).pending();
        assert!(reg.get_active_progress_bar().is_none());

        ProgressBar::set_active(&bar1, &reg);
        assert!(Arc::ptr_eq(&reg.get_active_progress_bar().unwrap(), &bar1));

        let bar2 = Builder::new()
            .registry(&reg)
            .thread_local_parent()
            .pending();
        ProgressBar::set_active(&bar2, &reg);
        assert!(Arc::ptr_eq(&reg.get_active_progress_bar().unwrap(), &bar2));

        ProgressBar::pop_active(&bar2, &reg);
        assert!(Arc::ptr_eq(&reg.get_active_progress_bar().unwrap(), &bar1));

        ProgressBar::pop_active(&bar1, &reg);
        assert!(&reg.get_active_progress_bar().is_none());
    }

    #[test]
    fn test_active_bar_out_of_order() {
        let reg = Registry::default();

        let bar1 = Builder::new().registry(&reg).active();
        let bar2 = Builder::new().registry(&reg).active();
        let bar3 = Builder::new().registry(&reg).active();

        // Pop out of order.
        drop(bar2);

        // bar3 is still active.
        assert!(Arc::ptr_eq(&reg.get_active_progress_bar().unwrap(), &bar3));

        drop(bar3);

        // bar1 becomes active since bar2 is already finished.
        assert!(Arc::ptr_eq(&reg.get_active_progress_bar().unwrap(), &bar1));

        drop(bar1);

        assert!(reg.get_active_progress_bar().is_none());
    }
}

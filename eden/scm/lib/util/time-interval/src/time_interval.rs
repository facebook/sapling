/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Logic to deal with time intervals: overlap, count, subtraction.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Instant;

use crate::spanset::Span;
use crate::spanset::SpanSet;

pub type CowStr = Cow<'static, str>;

/// Structure to track time intervals.
#[derive(Clone)]
pub struct TimeInterval {
    inner: Arc<RwLock<Inner>>,
}

/// Scoped interval for a tagged "blocking" operation.
pub struct BlockedInterval {
    tag: CowStr,
    start: u64,
    parent: TimeInterval,
}

struct Inner {
    // Start time of creating the `TimeInterval`
    start: Instant,
    // Track the "blocking" interval.
    total: SpanSet,
    // Track the intervals of different blocking intervals.
    tagged: HashMap<CowStr, SpanSet>,
    // Intervals smaller than this are ignored.
    ignore_threshold_ms: u64,

    #[cfg(test)]
    elapsed_override: Option<u64>,
}

impl TimeInterval {
    /// Creates the `TimeInterval` with `start` set to the current time.
    pub fn from_now() -> Self {
        let inner = Inner {
            start: Instant::now(),
            total: Default::default(),
            tagged: Default::default(),
            ignore_threshold_ms: 10,
            #[cfg(test)]
            elapsed_override: None,
        };
        Self {
            inner: Arc::new(RwLock::new(inner)),
        }
    }

    /// Elapsed time in milliseconds.
    pub fn elapsed_ms(&self) -> u64 {
        self.inner.read().unwrap().elapsed_ms()
    }

    /// Total blocked time in milliseconds.
    pub fn total_blocked_ms(&self) -> u64 {
        self.inner.read().unwrap().total.count()
    }

    /// Blocked time in milliseconds, for a given tag.
    pub fn tagged_blocked_ms(&self, tag: &str) -> u64 {
        let inner = self.inner.read().unwrap();
        match inner.tagged.get(tag) {
            Some(v) => v.count(),
            None => 0,
        }
    }

    /// Track a "blocked" interval.
    pub fn scoped_blocked_interval(&self, tag: CowStr) -> BlockedInterval {
        let start = self.elapsed_ms();
        BlockedInterval {
            tag,
            start,
            parent: self.clone(),
        }
    }

    /// List all tags.
    pub fn list_tags(&self) -> Vec<CowStr> {
        let mut tags: Vec<CowStr> = self.inner.read().unwrap().tagged.keys().cloned().collect();
        tags.sort_unstable();
        tags
    }
}

impl Inner {
    fn elapsed_ms(&self) -> u64 {
        #[cfg(test)]
        if let Some(v) = self.elapsed_override {
            return v;
        }
        self.start.elapsed().as_millis() as _
    }
}

impl Drop for BlockedInterval {
    fn drop(&mut self) {
        let mut inner = match self.parent.inner.write() {
            Ok(v) => v,
            _ => return,
        };
        let end = inner.elapsed_ms();
        if end - self.start > inner.ignore_threshold_ms {
            let span = Span::new(self.start, end - 1);
            inner.total.push(span);
            inner.tagged.entry(self.tag.clone()).or_default().push(span);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl TimeInterval {
        // "wait" for milliseconds.
        fn wait(&self, ms: u64) {
            let mut inner = self.inner.write().unwrap();
            inner.elapsed_override = Some(inner.elapsed_override.unwrap_or_default() + ms);
        }
    }

    #[test]
    fn test_empty() {
        let t = TimeInterval::from_now();
        t.wait(0);
        assert_eq!(t.elapsed_ms(), 0);
        assert_eq!(t.total_blocked_ms(), 0);
        assert_eq!(t.tagged_blocked_ms("foo"), 0);
    }

    #[test]
    fn test_single_span() {
        let t = TimeInterval::from_now();
        t.wait(100);
        let span = t.scoped_blocked_interval("foo".into());
        t.wait(200);
        drop(span);
        t.wait(100);
        assert_eq!(t.elapsed_ms(), 400);
        assert_eq!(t.total_blocked_ms(), 200);
        assert_eq!(t.tagged_blocked_ms("foo"), 200);
        assert_eq!(t.tagged_blocked_ms("bar"), 0);
    }

    #[test]
    fn test_overlap_spans() {
        let t = TimeInterval::from_now();
        t.wait(100);
        let span1 = t.scoped_blocked_interval("foo".into());
        t.wait(50);
        let span2 = t.scoped_blocked_interval("bar".into());
        t.wait(50);
        drop(span1);
        t.wait(50);
        drop(span2);
        t.wait(50);
        assert_eq!(t.elapsed_ms(), 300);
        assert_eq!(t.total_blocked_ms(), 150);
        assert_eq!(t.tagged_blocked_ms("foo"), 100);
        assert_eq!(t.tagged_blocked_ms("bar"), 100);
        assert_eq!(t.tagged_blocked_ms("baz"), 0);
        assert_eq!(t.list_tags(), vec!["bar", "foo"]);
    }

    #[test]
    fn test_duplicated_spans() {
        let t = TimeInterval::from_now();
        t.wait(100);
        let span1 = t.scoped_blocked_interval("foo".into());
        let span2 = t.scoped_blocked_interval("foo".into());
        t.wait(100);
        drop(span1);
        drop(span2);
        t.wait(100);
        assert_eq!(t.elapsed_ms(), 300);
        assert_eq!(t.total_blocked_ms(), 100);
        assert_eq!(t.tagged_blocked_ms("foo"), 100);
    }

    #[test]
    fn test_ignore_small_intervals() {
        let t = TimeInterval::from_now();
        t.wait(100);
        let span1 = t.scoped_blocked_interval("foo".into());
        t.wait(1);
        drop(span1);
        assert_eq!(t.elapsed_ms(), 101);
        assert_eq!(t.total_blocked_ms(), 0);
    }
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::fmt;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

/// Statistics for cache data. For example, "files: hit 1000, miss 2".
pub struct CacheStats {
    /// Topic (ex. "Files").
    topic: Cow<'static, str>,

    /// Total cache hit.
    hit: AtomicUsize,

    /// Total read.
    miss: AtomicUsize,
}

impl CacheStats {
    /// Create a [`CacheStats`].
    pub fn new(topic: impl Into<Cow<'static, str>>) -> Arc<Self> {
        let topic = topic.into();
        let stats = Self {
            topic,
            hit: Default::default(),
            miss: Default::default(),
        };
        Arc::new(stats)
    }

    /// The topic of the [`CacheStats`].
    pub fn topic(&self) -> &str {
        &self.topic
    }

    /// Increase the cache miss count.
    pub fn increase_miss(&self, count: usize) {
        self.miss.fetch_add(count, Relaxed);
    }

    /// Increase the cache hit count.
    pub fn increase_hit(&self, count: usize) {
        self.hit.fetch_add(count, Relaxed);
    }

    /// Get the cache miss count.
    pub fn miss(&self) -> usize {
        self.miss.load(Relaxed)
    }

    /// Get the cache hit count.
    pub fn hit(&self) -> usize {
        self.hit.load(Relaxed)
    }

    /// Get total read count.
    pub fn total(&self) -> usize {
        self.miss() + self.hit()
    }
}

impl fmt::Debug for CacheStats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "[{} hit {} miss {}]",
            &self.topic,
            self.hit(),
            self.miss()
        )
    }
}

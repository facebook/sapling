/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use fbinit::FacebookInit;
use stats::prelude::*;

define_stats! {
    prefix = "mononoke.git.server";
    // Per-repo counters for ShardManager scaling
    active_requests: dynamic_singleton_counter("{}.active_requests", (repo: String)),
    estimated_memory_bytes: dynamic_singleton_counter("{}.estimated_memory_bytes", (repo: String)),
}

/// Tracks request lifecycle and memory usage for git server requests.
///
/// This struct handles both:
/// 1. Active request counting (incremented on creation, decremented on drop)
/// 2. Memory weight tracking via the `WeightObserver` trait
///
/// Create using `WeightTracker::new()` and hold the Arc for the request lifetime.
/// Pass to `buffered_weighted` as an observer for memory tracking.
pub struct WeightTracker {
    fb: FacebookInit,
    repo_name: String,
}

impl std::fmt::Debug for WeightTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WeightTracker")
            .field("repo_name", &self.repo_name)
            .finish()
    }
}

impl WeightTracker {
    /// Create a new weight tracker for the given repository.
    ///
    /// This increments the active_requests counter. The counter will be
    /// decremented when the returned Arc is dropped (all references released).
    pub fn new(fb: FacebookInit, repo_name: String) -> Arc<Self> {
        // Increment active requests counter
        STATS::active_requests.increment_value(fb, 1, (repo_name.clone(),));
        Arc::new(Self { fb, repo_name })
    }
}

impl Drop for WeightTracker {
    fn drop(&mut self) {
        STATS::active_requests.increment_value(self.fb, -1, (self.repo_name.clone(),));
    }
}

// Implement the trait from buffered_weighted so WeightTracker can be used as an observer
impl buffered_weighted::WeightObserver for WeightTracker {
    fn on_weight_added(&self, weight: usize) {
        STATS::estimated_memory_bytes.increment_value(
            self.fb,
            weight as i64,
            (self.repo_name.clone(),),
        );
    }

    fn on_weight_removed(&self, weight: usize) {
        STATS::estimated_memory_bytes.increment_value(
            self.fb,
            -(weight as i64),
            (self.repo_name.clone(),),
        );
    }
}

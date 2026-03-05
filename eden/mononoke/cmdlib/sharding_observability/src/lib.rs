/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

pub use client_memory::ClientBucket;
pub use client_memory::ClientMemoryRegistry;
pub use client_memory::global_client_memory_registry;
use fbinit::FacebookInit;
use sharding_ext::encode_repo_name;
use stats::prelude::*;

define_stats! {
    prefix = "mononoke.git.server";
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
    client_bucket: ClientBucket,
}

impl std::fmt::Debug for WeightTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WeightTracker")
            .field("repo_name", &self.repo_name)
            .field("client_bucket", &self.client_bucket)
            .finish()
    }
}

impl WeightTracker {
    /// Create a new weight tracker for the given repository.
    ///
    /// This increments the active_requests counter. The counter will be
    /// decremented when the returned Arc is dropped (all references released).
    /// The repo name is encoded (e.g., `/` -> `_SLASH_`) so that the FB303
    /// counter keys match ShardManager's `[DOMAINID]` placeholder format.
    pub fn new(fb: FacebookInit, repo_name: String, main_client_id: Option<&str>) -> Arc<Self> {
        let encoded_name = encode_repo_name(&repo_name);
        STATS::active_requests.increment_value(fb, 1, (encoded_name.clone(),));
        Arc::new(Self {
            fb,
            repo_name: encoded_name,
            client_bucket: main_client_id.into(),
        })
    }

    /// Initialize FB303 counters for a repo when its shard is added to this host.
    /// Sets counters to 0 so SM can discover them immediately.
    pub fn on_shard_added(fb: FacebookInit, repo_name: &str) {
        let encoded_name = encode_repo_name(repo_name);
        STATS::active_requests.set_value(fb, 0, (encoded_name.clone(),));
        STATS::estimated_memory_bytes.set_value(fb, 0, (encoded_name,));
    }

    /// Clear FB303 counters for a repo when its shard is removed from this host.
    /// Ensures SM sees zero load for this repo on the old host immediately.
    pub fn on_shard_removed(fb: FacebookInit, repo_name: &str) {
        let encoded_name = encode_repo_name(repo_name);
        STATS::active_requests.set_value(fb, 0, (encoded_name.clone(),));
        STATS::estimated_memory_bytes.set_value(fb, 0, (encoded_name,));
    }
}

impl Drop for WeightTracker {
    fn drop(&mut self) {
        STATS::active_requests.increment_value(self.fb, -1, (self.repo_name.clone(),));
    }
}

impl weight_observer::WeightObserver for WeightTracker {
    fn on_weight_added(&self, weight: usize) {
        let w = weight as i64;
        STATS::estimated_memory_bytes.increment_value(self.fb, w, (self.repo_name.clone(),));
        global_client_memory_registry().add_weight(self.client_bucket, w);
    }

    fn on_weight_removed(&self, weight: usize) {
        let w = weight as i64;
        STATS::estimated_memory_bytes.increment_value(self.fb, -w, (self.repo_name.clone(),));
        global_client_memory_registry().remove_weight(self.client_bucket, w);
    }
}

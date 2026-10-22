/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use client_memory::ClientGroup;
pub use client_memory::ClientMemoryRegistry;
pub use client_memory::global_client_memory_registry;
use fbinit::FacebookInit;
use permission_checker::MononokeIdentitySet;
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
    client_group: ClientGroup,
}

impl std::fmt::Debug for WeightTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WeightTracker")
            .field("repo_name", &self.repo_name)
            .field("client_group", &self.client_group)
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
    pub fn new(
        fb: FacebookInit,
        repo_name: String,
        main_client_id: Option<&str>,
        identities: &MononokeIdentitySet,
    ) -> Arc<Self> {
        let encoded_name = encode_repo_name(&repo_name);
        STATS::active_requests.increment_value(fb, 1, (encoded_name.clone(),));
        Arc::new(Self {
            fb,
            repo_name: encoded_name,
            client_group: ClientGroup::for_request(Some(identities), main_client_id),
        })
    }

    /// Ensure FB303 counters are present for ShardManager to collect.
    ///
    /// A zero increment creates a missing counter at zero, including after a task
    /// restart, but preserves existing load held by requests across shard re-adds.
    /// Weight is released by its owning guards, not by shard removal.
    pub fn on_shard_added(fb: FacebookInit, repo_name: &str) {
        let encoded_name = encode_repo_name(repo_name);
        STATS::active_requests.increment_value(fb, 0, (encoded_name.clone(),));
        STATS::estimated_memory_bytes.increment_value(fb, 0, (encoded_name,));
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
        global_client_memory_registry().add_weight(self.client_group, w);
    }

    fn on_weight_removed(&self, weight: usize) {
        let w = weight as i64;
        STATS::estimated_memory_bytes.increment_value(self.fb, -w, (self.repo_name.clone(),));
        global_client_memory_registry().remove_weight(self.client_group, w);
    }
}

#[cfg(all(test, fbcode_build))]
mod tests {
    use std::thread;

    use mononoke_macros::mononoke;
    use permission_checker::MononokeIdentity;
    use weight_observer::WeightGuard;

    use super::*;

    fn counters(fb: FacebookInit, repo: &str) -> (Option<i64>, Option<i64>) {
        let encoded_name = encode_repo_name(repo);
        (
            STATS::active_requests.get_value(fb, (encoded_name.clone(),)),
            STATS::estimated_memory_bytes.get_value(fb, (encoded_name,)),
        )
    }

    #[mononoke::fbinit_test]
    fn shard_added_registers_zero_counters(fb: FacebookInit) {
        let repo = "weight_tracker_tests/register_zero";
        assert_eq!(counters(fb, repo), (None, None));

        WeightTracker::on_shard_added(fb, repo);
        assert_eq!(counters(fb, repo), (Some(0), Some(0)));
        WeightTracker::on_shard_added(fb, repo);
        assert_eq!(counters(fb, repo), (Some(0), Some(0)));
    }

    #[mononoke::fbinit_test]
    fn shard_added_preserves_request_started_before_setup(fb: FacebookInit) {
        let repo = "weight_tracker_tests/request_before_setup";
        let tracker = WeightTracker::new(fb, repo.to_owned(), None, &MononokeIdentitySet::new());
        let guard = WeightGuard::tracked(Some(tracker.clone()), 100);
        assert_eq!(counters(fb, repo), (Some(1), Some(100)));

        WeightTracker::on_shard_added(fb, repo);
        WeightTracker::on_shard_added(fb, repo);
        assert_eq!(counters(fb, repo), (Some(1), Some(100)));

        drop(tracker);
        assert_eq!(counters(fb, repo), (Some(1), Some(100)));
        drop(guard);
        assert_eq!(counters(fb, repo), (Some(0), Some(0)));
    }

    #[mononoke::fbinit_test]
    fn readding_shard_preserves_old_and_new_requests(fb: FacebookInit) {
        let repo = "weight_tracker_tests/readd";
        let identities: MononokeIdentitySet = [MononokeIdentity::from_legacy_type_data(
            "SERVICE_IDENTITY",
            "sandcastle.worker.warmed",
        )]
        .into_iter()
        .collect();
        let main_client_id = Some("SERVICE_IDENTITY:sandcastle.worker.warmed");
        let group = ClientGroup::Sandcastle;
        let registry = global_client_memory_registry();
        let baseline = registry.group_memory(group);

        WeightTracker::on_shard_added(fb, repo);
        let old_request = WeightTracker::new(fb, repo.to_owned(), main_client_id, &identities);
        assert_eq!(old_request.client_group, group);
        let mut old_guard = WeightGuard::tracked(Some(old_request), 100);
        assert_eq!(registry.group_memory(group), baseline + 100);

        WeightTracker::on_shard_added(fb, repo);
        assert_eq!(counters(fb, repo), (Some(1), Some(100)));
        assert_eq!(registry.group_memory(group), baseline + 100);
        let new_request = WeightTracker::new(fb, repo.to_owned(), main_client_id, &identities);
        assert_eq!(new_request.client_group, group);
        let new_guard = WeightGuard::tracked(Some(new_request), 40);
        assert_eq!(counters(fb, repo), (Some(2), Some(140)));
        assert_eq!(registry.group_memory(group), baseline + 140);

        old_guard.set_weight(150);
        assert_eq!(counters(fb, repo), (Some(2), Some(190)));
        assert_eq!(registry.group_memory(group), baseline + 190);
        WeightTracker::on_shard_added(fb, repo);
        assert_eq!(counters(fb, repo), (Some(2), Some(190)));
        assert_eq!(registry.group_memory(group), baseline + 190);

        drop(old_guard);
        assert_eq!(counters(fb, repo), (Some(1), Some(40)));
        assert_eq!(registry.group_memory(group), baseline + 40);
        drop(new_guard);
        assert_eq!(counters(fb, repo), (Some(0), Some(0)));
        assert_eq!(registry.group_memory(group), baseline);
    }

    #[mononoke::fbinit_test]
    fn request_stays_active_until_last_weight_guard_drops(fb: FacebookInit) {
        let repo = "weight_tracker_tests/last_guard";
        WeightTracker::on_shard_added(fb, repo);
        let tracker = WeightTracker::new(fb, repo.to_owned(), None, &MononokeIdentitySet::new());
        let index_guard = WeightGuard::tracked(Some(tracker.clone()), 222_000_000);
        let item_guard = WeightGuard::tracked(Some(tracker.clone()), 64);
        drop(tracker);

        WeightTracker::on_shard_added(fb, repo);
        assert_eq!(counters(fb, repo), (Some(1), Some(222_000_064)));
        drop(index_guard);
        assert_eq!(counters(fb, repo), (Some(1), Some(64)));
        drop(item_guard);
        assert_eq!(counters(fb, repo), (Some(0), Some(0)));
    }

    #[mononoke::fbinit_test]
    fn concurrent_registration_preserves_inflight_accounting(fb: FacebookInit) {
        let repo = "weight_tracker_tests/concurrent_registration";
        WeightTracker::on_shard_added(fb, repo);
        let request = WeightTracker::new(fb, repo.to_owned(), None, &MononokeIdentitySet::new());
        let guard = WeightGuard::tracked(Some(request), 100);

        thread::scope(|scope| {
            scope.spawn(|| {
                (0..100).for_each(|_| WeightTracker::on_shard_added(fb, repo));
            });
            scope.spawn(|| {
                let request =
                    WeightTracker::new(fb, repo.to_owned(), None, &MononokeIdentitySet::new());
                let mut guard = WeightGuard::tracked(Some(request), 40);
                (0..100).for_each(|_| {
                    guard.set_weight(80);
                    guard.set_weight(40);
                });
            });
        });

        assert_eq!(counters(fb, repo), (Some(1), Some(100)));
        drop(guard);
        assert_eq!(counters(fb, repo), (Some(0), Some(0)));
    }
}

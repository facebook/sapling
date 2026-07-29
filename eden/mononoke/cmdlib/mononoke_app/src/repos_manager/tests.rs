/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use config_reconcile::RepoGeneration;
use metaconfig_parser::RepoConfigs;
use metaconfig_parser::StorageConfigs;
use metaconfig_types::CommonConfig;
use metaconfig_types::RepoConfig;
use mononoke_configs::ConfigUpdateReceiver;
use mononoke_macros::mononoke;
use tokio::sync::Notify;

use super::ReconcileTrigger;
use super::apply_generation;
use super::memoized_spec_hash;
use super::reconcile_loop;
use super::retain_live_cache_entries;
use super::run_exclusive;
use super::tick_interval_secs;

#[mononoke::test]
fn test_tick_interval_off_uses_fixed_backstop() {
    // Off: fixed 60s, tunable value ignored (knob may be unregistered).
    assert_eq!(tick_interval_secs(false, 5), 60);
    assert_eq!(tick_interval_secs(false, 0), 60);
}

#[mononoke::test]
fn test_tick_interval_on_honors_knob_and_floors_zero() {
    assert_eq!(tick_interval_secs(true, 30), 30);
    assert_eq!(tick_interval_secs(true, 1), 1);
    // 0 must floor to 1s, never a busy loop.
    assert_eq!(tick_interval_secs(true, 0), 1);
}

#[mononoke::test]
async fn test_reconcile_trigger_wakes_on_bulk_update() {
    let notify = Arc::new(Notify::new());
    let trigger = ReconcileTrigger {
        notify: notify.clone(),
    };
    trigger
        .apply_update(
            Arc::new(RepoConfigs::new(HashMap::new(), CommonConfig::default())),
            Arc::new(StorageConfigs {
                storage: HashMap::new(),
            }),
        )
        .await
        .expect("apply_update is infallible");
    // notify_one leaves a permit, so notified() resolves at once; the timeout
    // only guards against a hang if the trigger failed to fire.
    tokio::time::timeout(Duration::from_secs(30), notify.notified())
        .await
        .expect("a bulk config update must wake the reconcile loop");
}

#[mononoke::test]
async fn test_reconcile_trigger_wakes_on_per_repo_update() {
    let notify = Arc::new(Notify::new());
    let trigger = ReconcileTrigger {
        notify: notify.clone(),
    };
    trigger
        .apply_repo_update("some_repo", &RepoConfig::default())
        .await
        .expect("apply_repo_update is infallible");
    tokio::time::timeout(Duration::from_secs(30), notify.notified())
        .await
        .expect("a per-repo config update must wake the reconcile loop");
}

// --- memoized_spec_hash ----------------------------------------------------
//
// A tiny `i32` stands in for `RepoSpec`; the injected `compute` treats the value
// itself as the "hash", so two Arcs with the same value hash equal even though
// they are distinct allocations (distinct pointers).

#[mononoke::test]
fn test_memoized_spec_hash_steady_state_hit() {
    // Same Arc across calls: the cached hash is reused, so compute runs once.
    let cache: Mutex<HashMap<String, (Arc<i32>, u64)>> = Mutex::new(HashMap::new());
    let calls = AtomicUsize::new(0);
    let spec = Arc::new(7i32);

    let h1 = memoized_spec_hash(&cache, "repo", &spec, |s| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(*s as u64)
    });
    assert_eq!(h1, Some(7), "first call computes and returns the hash");
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let h2 = memoized_spec_hash(&cache, "repo", &spec, |s| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(*s as u64)
    });
    assert_eq!(h2, Some(7));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "pointer-identical Arc must reuse the cached hash without recomputing",
    );
}

#[mononoke::test]
fn test_memoized_spec_hash_noop_bump_recomputes_same_hash() {
    // A new Arc (different allocation) with the same value: the pointer miss
    // forces a recompute, but the resulting hash is unchanged (no-op content bump).
    let cache: Mutex<HashMap<String, (Arc<i32>, u64)>> = Mutex::new(HashMap::new());
    let calls = AtomicUsize::new(0);

    let spec1 = Arc::new(7i32);
    let h1 = memoized_spec_hash(&cache, "repo", &spec1, |s| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(*s as u64)
    });
    assert_eq!(h1, Some(7));

    let spec2 = Arc::new(7i32);
    assert!(
        !Arc::ptr_eq(&spec1, &spec2),
        "spec2 must be a distinct allocation",
    );
    let h2 = memoized_spec_hash(&cache, "repo", &spec2, |s| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(*s as u64)
    });
    assert_eq!(h2, Some(7), "same content must yield the same hash");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "a new Arc must recompute even when the hash is unchanged",
    );
}

#[mononoke::test]
fn test_memoized_spec_hash_real_change() {
    // A new Arc with a different value: recompute yields the new, different hash.
    let cache: Mutex<HashMap<String, (Arc<i32>, u64)>> = Mutex::new(HashMap::new());

    let spec1 = Arc::new(7i32);
    let h1 = memoized_spec_hash(&cache, "repo", &spec1, |s| Ok(*s as u64));
    assert_eq!(h1, Some(7));

    let spec2 = Arc::new(8i32);
    let h2 = memoized_spec_hash(&cache, "repo", &spec2, |s| Ok(*s as u64));
    assert_eq!(h2, Some(8), "changed content must produce the new hash");
}

#[mononoke::test]
fn test_memoized_spec_hash_caches_arc_strong_count() {
    // ABA proxy: after insert, the cache holds its own clone of the Arc, so the
    // strong count rises to >= 2 (our local + the cached one). Storing the Arc
    // (not a raw pointer) is what stops a reused address from false-matching.
    let cache: Mutex<HashMap<String, (Arc<i32>, u64)>> = Mutex::new(HashMap::new());
    let spec = Arc::new(7i32);
    assert_eq!(Arc::strong_count(&spec), 1);

    let _ = memoized_spec_hash(&cache, "repo", &spec, |s| Ok(*s as u64));
    assert!(
        Arc::strong_count(&spec) >= 2,
        "cache must retain its own clone of the Arc, got {}",
        Arc::strong_count(&spec),
    );
}

#[mononoke::test]
fn test_memoized_spec_hash_compute_err_returns_none() {
    // compute Err => None (matching the caller's `.ok()?`) and nothing is cached.
    let cache: Mutex<HashMap<String, (Arc<i32>, u64)>> = Mutex::new(HashMap::new());
    let spec = Arc::new(7i32);

    let h = memoized_spec_hash(&cache, "repo", &spec, |_| Err(anyhow::anyhow!("boom")));
    assert_eq!(h, None, "a failed compute must return None");
    assert!(
        cache.lock().expect("cache poisoned").is_empty(),
        "a failed compute must not populate the cache",
    );
}

// --- retain_live_cache_entries ---------------------------------------------

#[mononoke::test]
fn test_retain_live_cache_entries_evicts_absent() {
    // Seed {a,b,c}; keep {a,c}; b is evicted and its cached Arc released.
    let cache: Mutex<HashMap<String, (Arc<i32>, u64)>> = Mutex::new(HashMap::new());
    let spec_b = Arc::new(2i32);
    {
        let mut c = cache.lock().expect("cache poisoned");
        c.insert("a".to_string(), (Arc::new(1i32), 1));
        c.insert("b".to_string(), (spec_b.clone(), 2));
        c.insert("c".to_string(), (Arc::new(3i32), 3));
    }
    assert_eq!(
        Arc::strong_count(&spec_b),
        2,
        "local + cached clone before eviction",
    );

    let live: HashSet<&str> = ["a", "c"].into_iter().collect();
    retain_live_cache_entries(&cache, &live);

    let c = cache.lock().expect("cache poisoned");
    let mut names: Vec<&str> = c.keys().map(String::as_str).collect();
    names.sort();
    assert_eq!(names, vec!["a", "c"], "only live entries remain");
    drop(c);
    assert_eq!(
        Arc::strong_count(&spec_b),
        1,
        "evicting b must drop the cache's clone of its Arc",
    );
}

// --- run_exclusive ---------------------------------------------------------

#[mononoke::test]
async fn test_run_exclusive_runs_when_free() {
    let lock = tokio::sync::Mutex::new(());
    let ran = AtomicUsize::new(0);
    let out = run_exclusive(&lock, || async {
        ran.fetch_add(1, Ordering::SeqCst);
        42
    })
    .await;
    assert_eq!(out, Some(42), "free lock runs body and returns its output");
    assert_eq!(ran.load(Ordering::SeqCst), 1);
}

#[mononoke::test]
async fn test_run_exclusive_skips_when_held() {
    let lock = tokio::sync::Mutex::new(());
    let guard = lock.lock().await; // hold the lock
    let ran = AtomicUsize::new(0);

    // try_lock fails, so this returns promptly without queuing.
    let out = run_exclusive(&lock, || async {
        ran.fetch_add(1, Ordering::SeqCst);
        42
    })
    .await;
    assert_eq!(out, None, "a held lock must skip (not queue)");
    assert_eq!(ran.load(Ordering::SeqCst), 0, "skipped body must not run");
    drop(guard);
}

#[mononoke::test]
async fn test_run_exclusive_reruns_after_release() {
    let lock = tokio::sync::Mutex::new(());
    let ran = AtomicUsize::new(0);
    {
        let guard = lock.lock().await;
        let skipped = run_exclusive(&lock, || async {
            ran.fetch_add(1, Ordering::SeqCst);
            1
        })
        .await;
        assert_eq!(skipped, None, "skipped while held");
        drop(guard);
    }
    let out = run_exclusive(&lock, || async {
        ran.fetch_add(1, Ordering::SeqCst);
        1
    })
    .await;
    assert_eq!(out, Some(1), "must run once the lock is free again");
    assert_eq!(
        ran.load(Ordering::SeqCst),
        1,
        "only the post-release run fired"
    );
}

#[mononoke::test]
async fn test_run_exclusive_sequential_calls_both_run() {
    // No contention between sequential calls: each acquires and releases the lock.
    let lock = tokio::sync::Mutex::new(());
    let ran = AtomicUsize::new(0);
    let a = run_exclusive(&lock, || async {
        ran.fetch_add(1, Ordering::SeqCst);
    })
    .await;
    let b = run_exclusive(&lock, || async {
        ran.fetch_add(1, Ordering::SeqCst);
    })
    .await;
    assert_eq!(a, Some(()));
    assert_eq!(b, Some(()));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        2,
        "both sequential (non-contending) calls run",
    );
}

// --- reconcile_loop --------------------------------------------------------
//
// Virtual time (`tokio::time::pause`) drives the backstop; the pass closure
// signals a `passed` Notify after each run so the test can await exactly one pass
// per phase (rather than guess yield counts). The backstop advance goes just past
// the interval so the sleep deadline is definitely reached. The injected
// `next_interval` keeps justknobs out of the loop.

/// Spawn a `reconcile_loop` whose pass bumps `count` then fires `passed`.
fn spawn_counting_loop(
    count: Arc<AtomicUsize>,
    passed: Arc<Notify>,
    trigger: Arc<Notify>,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(reconcile_loop(
        move || {
            let count = count.clone();
            let passed = passed.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                passed.notify_one();
            }
        },
        trigger,
        move || interval,
    ))
}

#[mononoke::test]
async fn test_reconcile_loop_reconcile_first_and_wakes() {
    tokio::time::pause();
    let count = Arc::new(AtomicUsize::new(0));
    let passed = Arc::new(Notify::new());
    let trigger = Arc::new(Notify::new());
    let interval = Duration::from_secs(60);

    let handle = spawn_counting_loop(count.clone(), passed.clone(), trigger.clone(), interval);

    // Reconcile-first: one pass runs before the loop ever waits.
    passed.notified().await;
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "loop must run a pass before waiting",
    );

    // A trigger notification wakes it for another pass.
    trigger.notify_one();
    passed.notified().await;
    assert_eq!(
        count.load(Ordering::SeqCst),
        2,
        "a trigger notification must run another pass",
    );

    // The backstop sleep wakes it: advance just past the interval to fire it.
    tokio::time::advance(interval + Duration::from_millis(1)).await;
    passed.notified().await;
    assert_eq!(
        count.load(Ordering::SeqCst),
        3,
        "the backstop sleep must run another pass",
    );

    handle.abort();
}

#[mononoke::test]
async fn test_reconcile_loop_abort_stops() {
    tokio::time::pause();
    let count = Arc::new(AtomicUsize::new(0));
    let passed = Arc::new(Notify::new());
    let trigger = Arc::new(Notify::new());
    let interval = Duration::from_secs(60);

    let handle = spawn_counting_loop(count.clone(), passed.clone(), trigger.clone(), interval);

    // Let the reconcile-first pass run, then stop the loop.
    passed.notified().await;
    assert_eq!(count.load(Ordering::SeqCst), 1);
    handle.abort();
    tokio::task::yield_now().await;

    // Neither a trigger nor the backstop runs a pass after abort. There is no
    // pass to await, so drive the scheduler and assert the count is unchanged.
    trigger.notify_one();
    tokio::time::advance(interval + Duration::from_millis(1)).await;
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "an aborted loop must not run further passes",
    );
}

// --- apply_generation ------------------------------------------------------

#[mononoke::test]
fn test_apply_generation_truth_table() {
    let generation = RepoGeneration {
        spec_hash: 11,
        storage_gen: 22,
    };

    // Shallow reload always applies, so a generation is always recorded.
    assert_eq!(apply_generation(false, true, generation), Some(generation));
    // Deep reload that hit a present repo records the generation.
    assert_eq!(apply_generation(true, true, generation), Some(generation));
    // Deep reload of a not-present repo (reload_if_present == false) records none.
    assert_eq!(
        apply_generation(true, false, generation),
        None,
        "a deep repo that was not present must not record a generation",
    );
}

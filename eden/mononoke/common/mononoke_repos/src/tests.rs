/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Unit tests for `MononokeRepos`. `mod tests;` submodule so `super` is the crate root.

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use futures::FutureExt;
use mononoke_macros::mononoke;

use super::*;

/// Loads `42`, and counts how often it was asked to.
#[derive(Default)]
struct CountingLoader {
    calls: AtomicUsize,
}

impl RepoLoader<i32> for CountingLoader {
    fn load(&self, _repo_name: String) -> BoxFuture<'static, Result<i32>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        async { Ok(42) }.boxed()
    }
}

/// For the cases answerable without a load: already there, or never assigned.
struct NeverLoads;

impl RepoLoader<i32> for NeverLoads {
    fn load(&self, repo_name: String) -> BoxFuture<'static, Result<i32>> {
        panic!("must not load {repo_name}")
    }
}

#[mononoke::test]
fn test_reload_if_present() {
    let repos: MononokeRepos<i32> = MononokeRepos::new();
    repos.add("foo", 1, 100);
    assert_eq!(repos.get_by_name("foo").as_deref(), Some(&100));

    // Present -> replaced.
    assert!(repos.reload_if_present(1, "foo".to_string(), 200));
    assert_eq!(repos.get_by_name("foo").as_deref(), Some(&200));

    // Absent -> no-op; must not be resurrected.
    assert!(!repos.reload_if_present(2, "bar".to_string(), 300));
    assert!(repos.get_by_name("bar").is_none());
}

#[mononoke::test]
fn test_placeholder_is_assigned_but_not_loaded() {
    let repos: MononokeRepos<i32> = MononokeRepos::new();
    repos.add_placeholder("foo", 1);

    // Assigned: the name and id are known to the service.
    assert_eq!(repos.iter_names().collect::<Vec<_>>(), vec!["foo"]);
    assert_eq!(repos.iter_ids().collect::<Vec<_>>(), vec![1]);

    // Not loaded: every repo accessor behaves as though it is absent.
    assert!(repos.get_by_name("foo").is_none());
    assert!(repos.get_by_id(1).is_none());
    assert_eq!(repos.iter().count(), 0);
}

#[mononoke::test]
fn test_placeholder_becomes_visible_once_built() {
    let repos: MononokeRepos<i32> = MononokeRepos::new();
    repos.add_placeholder("foo", 1);
    assert!(repos.get_by_name("foo").is_none());

    repos.add("foo", 1, 100);
    assert_eq!(repos.get_by_name("foo").as_deref(), Some(&100));
    assert_eq!(repos.get_by_id(1).as_deref(), Some(&100));
    assert_eq!(repos.iter().count(), 1);

    // The placeholder did not leave a duplicate entry behind.
    assert_eq!(repos.iter_names().count(), 1);
}

#[mononoke::test]
fn test_placeholder_counts_as_present_for_reload_if_present() {
    let repos: MononokeRepos<i32> = MononokeRepos::new();
    repos.add_placeholder("foo", 1);

    // A placeholder is an assigned repo, so a rebuild targeting it is not a
    // resurrection and must be applied.
    assert!(repos.reload_if_present(1, "foo".to_string(), 100));
    assert_eq!(repos.get_by_name("foo").as_deref(), Some(&100));
}

#[mononoke::test]
fn test_add_placeholder_does_not_unbuild_a_loaded_repo() {
    let repos: MononokeRepos<i32> = MononokeRepos::new();
    repos.add("foo", 1, 100);

    // An assignment arriving for a repo that is already built must leave it
    // alone. Un-building is eviction, which this type does not do.
    repos.add_placeholder("foo", 1);

    assert_eq!(repos.get_by_name("foo").as_deref(), Some(&100));
    assert_eq!(repos.get_by_id(1).as_deref(), Some(&100));
    assert_eq!(repos.iter().count(), 1);
}

#[mononoke::test]
fn test_add_placeholder_twice_leaves_one_assignment() {
    let repos: MononokeRepos<i32> = MononokeRepos::new();
    repos.add_placeholder("foo", 1);
    repos.add_placeholder("foo", 1);

    assert_eq!(repos.iter_names().count(), 1);
    assert_eq!(repos.iter_ids().count(), 1);
    assert!(repos.get_by_name("foo").is_none());
}

#[mononoke::test]
fn test_iter_loaded_names_excludes_a_placeholder() {
    let repos: MononokeRepos<i32> = MononokeRepos::new();
    repos.add("built", 1, 100);
    repos.add_placeholder("assigned", 2);

    let mut assigned: Vec<_> = repos.iter_names().collect();
    assigned.sort();
    assert_eq!(
        assigned,
        vec!["assigned", "built"],
        "iter_names reports assignment, so both are listed"
    );

    assert_eq!(
        repos.iter_loaded_names().collect::<Vec<_>>(),
        vec!["built"],
        "an assigned-but-unbuilt repo must not be reported as loaded"
    );
}

#[mononoke::test]
fn test_iter_loaded_names_includes_a_placeholder_once_built() {
    let repos: MononokeRepos<i32> = MononokeRepos::new();
    repos.add_placeholder("foo", 1);
    assert_eq!(repos.iter_loaded_names().count(), 0);

    repos.add("foo", 1, 100);
    assert_eq!(repos.iter_loaded_names().collect::<Vec<_>>(), vec!["foo"]);
}

#[mononoke::test]
fn test_remove_drops_a_placeholder() {
    let repos: MononokeRepos<i32> = MononokeRepos::new();
    repos.add_placeholder("foo", 1);
    repos.remove("foo");

    assert_eq!(repos.iter_names().count(), 0);
    assert_eq!(repos.iter_ids().count(), 0);
    assert!(!repos.reload_if_present(1, "foo".to_string(), 100));
}

#[mononoke::test]
async fn test_an_unassigned_repo_is_absent_rather_than_an_error() {
    let repos: MononokeRepos<i32> = MononokeRepos::new_lazy(Arc::new(NeverLoads));

    assert!(
        repos.get("nope").await.unwrap().is_none(),
        "a repo this service was never assigned is absent, not an error"
    );
}

#[mononoke::test]
async fn test_a_built_repo_is_served_without_building() {
    let repos: MononokeRepos<i32> = MononokeRepos::new_lazy(Arc::new(NeverLoads));
    repos.add("foo", 1, 100);

    let repo = repos.get("foo").await.unwrap().expect("foo is built");
    assert_eq!(*repo, 100);
}

#[mononoke::test]
async fn test_an_unbuilt_repo_is_absent_without_a_loader() {
    let repos: MononokeRepos<i32> = MononokeRepos::new();
    repos.add_placeholder("foo", 1);

    // Pins the absence of a second error state: a caller never has to tell
    // this apart from a routing problem.
    assert!(repos.get("foo").await.unwrap().is_none());
}

#[mononoke::test]
async fn test_an_unbuilt_repo_is_built_on_first_request() {
    let loader = Arc::new(CountingLoader::default());
    let repos: MononokeRepos<i32> = MononokeRepos::new_lazy(loader.clone());
    repos.add_placeholder("foo", 1);

    let repo = repos.get("foo").await.unwrap().expect("foo is assigned");
    assert_eq!(*repo, 42);
    assert_eq!(loader.calls.load(Ordering::SeqCst), 1);

    // The second request is served from the slot the first one filled.
    let repo = repos.get("foo").await.unwrap().expect("foo is built now");
    assert_eq!(*repo, 42);
    assert_eq!(
        loader.calls.load(Ordering::SeqCst),
        1,
        "a repo that is already built must not be built again"
    );
}

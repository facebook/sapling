/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Unit tests for `MononokeRepos`. `mod tests;` submodule so `super` is the crate root.

use mononoke_macros::mononoke;

use super::*;

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

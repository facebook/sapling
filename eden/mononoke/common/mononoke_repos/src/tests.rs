/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Unit tests for `MononokeRepos`. `mod tests;` submodule so `super` is the crate root.

use super::*;

#[test]
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

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_macros::mononoke;

use super::*;

fn failure(name: &str) -> (String, anyhow::Error) {
    (name.to_string(), anyhow::anyhow!("boom"))
}

// The scope fix: only repos this task serves at boot (non-deep-sharded per the
// manifest AND matching the repo filter) may block startup. Deep-sharded repos
// load on demand and filtered-out repos are never served, so their parse
// failures must be excluded from the fail-closed set.
#[mononoke::test]
fn blocking_failures_excludes_deep_and_filtered() {
    let failed = vec![
        failure("shallow_a"),
        failure("deep_b"),
        failure("filtered_c"),
    ];

    // Manifest marks a and c as non-deep-sharded; b is deep-sharded.
    let non_deep = ["shallow_a", "filtered_c"];
    // The repo filter excludes c (this task does not serve it).
    let blocking =
        startup_blocking_failures(&failed, |n| non_deep.contains(&n), |n| n != "filtered_c");

    let names: Vec<&str> = blocking.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        vec!["shallow_a"],
        "only the served (non-deep, filter-matching) repo must block startup"
    );
}

// Whole-tier (service == None) callers serve every enabled repo, so the
// serves-all deep predicate `|_| true` must NOT exclude a deep repo's failure.
#[mononoke::test]
fn blocking_failures_serves_all_includes_deep() {
    let failed = vec![failure("shallow_a"), failure("deep_b")];

    // serves-all: no repo excluded as deep, no repo filtered out.
    let blocking = startup_blocking_failures(&failed, |_| true, |_| true);

    let names: Vec<&str> = blocking.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        vec!["shallow_a", "deep_b"],
        "whole-tier callers serve all repos, so a deep repo's failure must block startup"
    );
}

// No blocking failures -> Ok (nothing this task serves failed to parse).
#[mononoke::test]
fn decision_empty_is_ok() {
    assert!(startup_parse_failure_decision(&[]).is_ok());
}

// Fail-closed must list every blocking repo, not just the first.
#[mononoke::test]
fn decision_non_empty_errors_listing_all() {
    let failed = [failure("repo/one"), failure("repo/two")];
    let blocking: Vec<&(String, anyhow::Error)> = failed.iter().collect();

    let err = startup_parse_failure_decision(&blocking)
        .expect_err("must fail closed when served repos failed to parse");
    let msg = format!("{err:#}");
    assert!(msg.contains("repo/one"), "message missing repo/one: {msg}");
    assert!(msg.contains("repo/two"), "message missing repo/two: {msg}");
}

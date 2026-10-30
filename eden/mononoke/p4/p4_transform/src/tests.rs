/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_macros::mononoke;
use mononoke_types::hash::Blake3;
use mononoke_types::hash::RichGitSha1;
use mononoke_types::hash::Sha1;
use mononoke_types::hash::Sha256;
use mononoke_types_mocks::contentid::ONES_CTID;

use super::*;

const DEPOT: &str = "//test1/";

/// Only `content_id` and `total_size` make it into a file change.
fn content() -> ContentMetadataV2 {
    ContentMetadataV2 {
        content_id: ONES_CTID,
        total_size: 3,
        sha1: Sha1::from_byte_array([0; 20]),
        sha256: Sha256::from_byte_array([0; 32]),
        git_sha1: RichGitSha1::from_byte_array([0; 20], "blob", 3),
        is_binary: false,
        is_ascii: true,
        is_utf8: true,
        ends_in_newline: false,
        newline_count: 0,
        first_line: None,
        is_generated: false,
        is_partially_generated: false,
        seeded_blake3: Blake3::from_byte_array([0; 32]),
    }
}

fn revision(depot_path: &str, action: Action) -> FileRevision {
    FileRevision {
        depot_path: depot_path.to_owned(),
        action,
        file_type: "text".to_owned(),
        revision: 1,
        size: None,
        digest: None,
    }
}

fn change_of(
    depot_path: &str,
    action: Action,
    content: Option<&ContentMetadataV2>,
) -> Result<Option<(NonRootMPath, FileChange)>, TransformError> {
    file_change(DEPOT, &revision(depot_path, action), content)
}

fn path_of(depot_path: &str) -> String {
    let (path, _) = change_of(depot_path, Action::Delete, None)
        .unwrap()
        .unwrap();
    path.to_string()
}

fn regular() -> FileChange {
    FileChange::tracked(ONES_CTID, FileType::Regular, 3, None, GitLfs::FullContent)
}

#[mononoke::test]
fn test_content_actions_are_changes_with_the_stored_content() {
    for action in [
        Action::Add,
        Action::Edit,
        Action::Import,
        Action::Integrate,
        Action::Branch,
        Action::MoveAdd,
    ] {
        let (path, change) = change_of("//test1/a.txt", action, Some(&content()))
            .unwrap()
            .unwrap();
        assert_eq!("a.txt", path.to_string(), "{action:?}");
        assert_eq!(regular(), change, "{action:?}");
    }
}

#[mononoke::test]
fn test_actions_without_content_are_deletions() {
    for action in [
        Action::Delete,
        Action::MoveDelete,
        Action::Purge,
        Action::Archive,
    ] {
        let (_, change) = change_of("//test1/a.txt", action, None).unwrap().unwrap();
        assert_eq!(FileChange::Deletion, change, "{action:?}");
    }
}

#[mononoke::test]
fn test_content_action_without_content_is_an_error() {
    let result = change_of("//test1/a.txt", Action::Edit, None);
    assert!(
        matches!(&result, Err(TransformError::MissingContent(p)) if p == "//test1/a.txt"),
        "{result:?}"
    );
}

#[mononoke::test]
fn test_paths_lose_the_depot_root_and_keep_their_case() {
    assert_eq!("Dir/README.md", path_of("//test1/Dir/README.md"));
}

#[mononoke::test]
fn test_files_in_other_depots_have_no_change() {
    for depot_path in ["//test10/a.txt", "//other/a.txt", "//TEST1/a.txt"] {
        let result = change_of(depot_path, Action::Delete, None);
        assert!(matches!(result, Ok(None)), "{depot_path}: {result:?}");
    }
}

#[mononoke::test]
fn test_depot_root_that_is_not_one_depot_is_an_error() {
    for depot_root in ["//test1", "//", "///", "//a/b/", "test1/"] {
        let result = file_change(depot_root, &revision("//test1/a.txt", Action::Delete), None);
        assert!(
            matches!(&result, Err(TransformError::InvalidDepotRoot(d)) if d == depot_root),
            "{result:?}"
        );
    }
}

#[mononoke::test]
fn test_path_mononoke_rejects_is_an_error() {
    for depot_path in ["//test1/", "//test1/a/../b"] {
        let result = change_of(depot_path, Action::Delete, None);
        assert!(
            matches!(&result, Err(TransformError::InvalidPath { depot_path: p, .. }) if p == depot_path),
            "{result:?}"
        );
    }
}

#[mononoke::test]
fn test_paths_are_unescaped() {
    for (depot_path, path) in [
        ("//test1/foo%402x.png", "foo@2x.png"),
        ("//test1/issue%2312.txt", "issue#12.txt"),
        ("//test1/100%25.txt", "100%.txt"),
        ("//test1/star%2A.txt", "star*.txt"),
        ("//test1/literal%2540.txt", "literal%40.txt"),
    ] {
        assert_eq!(path, path_of(depot_path), "{depot_path}");
    }
}

#[mononoke::test]
fn test_unescape_leaves_other_percent_sequences() {
    assert_eq!("a%2Fb%2a%", unescape_path("a%2Fb%2a%"));
}

#[mononoke::test]
fn test_escape_and_unescape_round_trip() {
    for (path, depot_path) in [
        ("plain/name.txt", "plain/name.txt"),
        ("foo@2x.png", "foo%402x.png"),
        ("issue#12.txt", "issue%2312.txt"),
        ("100%.txt", "100%25.txt"),
        ("star*.txt", "star%2A.txt"),
        ("literal%40.txt", "literal%2540.txt"),
        ("@#%*", "%40%23%25%2A"),
    ] {
        assert_eq!(depot_path, escape_path(path), "{path}");
        assert_eq!(path, unescape_path(depot_path), "{depot_path}");
    }
}

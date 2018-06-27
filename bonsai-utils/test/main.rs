// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate futures;
#[macro_use]
extern crate pretty_assertions;

extern crate async_unit;

extern crate bonsai_utils;
extern crate mercurial_types;
extern crate mercurial_types_mocks;
extern crate mononoke_types;

mod fixtures;

use futures::{Future, Stream};

use async_unit::tokio_unit_test;

use bonsai_utils::{bonsai_diff, BonsaiDiffResult};
use mercurial_types::{Entry, HgEntryId};
use mercurial_types_mocks::manifest::{MockEntry, MockManifest};
use mercurial_types_mocks::nodehash::*;
use mononoke_types::{FileType, MPath, RepoPath, path::check_pcf};

use fixtures::ManifestFixture;

#[test]
fn diff_basic() {
    tokio_unit_test(|| {
        let parent_entry = root_entry(&fixtures::BASIC1);
        let working_entry = root_entry(&fixtures::BASIC2);

        let diff = compute_diff(working_entry, Some(parent_entry), None);
        let expected_diff = vec![
            diff_result("dir1/file-to-dir", None),
            // dir1/file-to-dir/foobar *is* a result, because it has changed and its parent is
            // deleted.
            diff_result(
                "dir1/file-to-dir/foobar",
                Some((FileType::Symlink, THREES_EID)),
            ),
            diff_result("dir1/foo", Some((FileType::Regular, THREES_EID))),
            diff_result("dir2/bar", None),
            diff_result("dir2/dir-to-file", Some((FileType::Executable, DS_EID))),
            // dir2/dir-to-file/foo is *not* a result, because its parent is marked changed
            diff_result(
                "dir2/only-file-type",
                Some((FileType::Executable, ONES_EID)),
            ),
            diff_result("dir2/quux", Some((FileType::Symlink, FOURS_EID))),
        ];

        assert_eq!(diff, expected_diff);

        // Test out multiple parents with the same hashes.
        let parent1 = root_entry(&fixtures::BASIC1);
        let parent2 = root_entry(&fixtures::BASIC1);
        let working_entry = root_entry(&fixtures::BASIC2);

        let diff = compute_diff(working_entry, Some(parent1), Some(parent2));
        assert_eq!(diff, expected_diff);
    })
}

#[test]
fn diff_truncate() {
    tokio_unit_test(|| {
        let parent_entry = root_entry(&fixtures::TRUNCATE1);
        let working_entry = root_entry(&fixtures::TRUNCATE2);

        let diff = bonsai_diff(working_entry, Some(parent_entry), None);
        let paths = diff.collect().wait().expect("computing diff failed");
        assert_eq!(paths, vec![]);
    })
}

#[test]
fn diff_merge1() {
    tokio_unit_test(|| {
        let parent1 = root_entry(&fixtures::BASIC1);
        let parent2 = root_entry(&fixtures::BASIC2);
        let working_entry = root_entry(&fixtures::BASIC2);

        let diff = compute_diff(working_entry, Some(parent1), Some(parent2));

        // Compare this result to expected_diff in diff_basic.
        let expected_diff = vec![
            diff_result("dir1/file-to-dir", None),
            // dir1/file-to-dir/foobar is *not* a result because p1 doesn't have it and p2 has the
            // same contents.
            diff_result("dir1/foo", Some((FileType::Regular, THREES_EID))),
            diff_result("dir2/bar", None),
            diff_result("dir2/dir-to-file", Some((FileType::Executable, DS_EID))),
            diff_result(
                "dir2/only-file-type",
                Some((FileType::Executable, ONES_EID)),
            ),
            // dir2/quux is not a result because it isn't present in p1 and is present in p2, so
            // the version from p2 is implicitly chosen.
        ];
        assert_eq!(diff, expected_diff);
    })
}

fn root_entry(mf: &ManifestFixture) -> Box<Entry + Sync> {
    let path_hashes = mf.path_hashes.iter().cloned();
    let dir_hashes = mf.dir_hashes.iter().cloned();
    let mock_manifest =
        MockManifest::from_path_hashes(path_hashes, dir_hashes).expect("valid manifest");
    let mut entry = MockEntry::from_manifest(RepoPath::RootPath, mock_manifest);
    entry.set_hash(mf.root_hash);
    entry.boxed()
}

fn compute_diff(
    working_entry: Box<Entry + Sync>,
    p1_entry: Option<Box<Entry + Sync>>,
    p2_entry: Option<Box<Entry + Sync>>,
) -> Vec<BonsaiDiffResult> {
    let diff_stream = bonsai_diff(working_entry, p1_entry, p2_entry);
    let mut paths = diff_stream.collect().wait().expect("computing diff failed");
    paths.sort_unstable();

    check_pcf(paths.iter().map(|diff_result| match diff_result {
        BonsaiDiffResult::Changed(path, ..) => (path, true),
        BonsaiDiffResult::Deleted(path) => (path, false),
    })).expect("paths must be path-conflict-free");

    // TODO: check that the result is path-conflict-free
    paths
}

fn diff_result<P>(path: P, details: Option<(FileType, HgEntryId)>) -> BonsaiDiffResult
where
    P: AsRef<[u8]>,
{
    let path = MPath::new(path).expect("valid path");
    match details {
        Some((ft, hash)) => BonsaiDiffResult::Changed(path, ft, hash),
        None => BonsaiDiffResult::Deleted(path),
    }
}

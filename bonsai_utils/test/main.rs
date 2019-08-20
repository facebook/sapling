// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use async_unit::tokio_unit_test;
use bonsai_utils::{bonsai_diff, BonsaiDiffResult};
use context::CoreContext;
use futures::{Future, Stream};
use mercurial_types::{HgEntry, HgFileNodeId};
use mercurial_types_mocks::manifest::{MockEntry, MockManifest};
use mercurial_types_mocks::nodehash::*;
use mononoke_types::{path::check_pcf, FileType, MPath, RepoPath};
use pretty_assertions::assert_eq;

mod fixtures;
use crate::fixtures::ManifestFixture;

#[test]
fn diff_basic() {
    tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let parent_entry = root_entry(&fixtures::BASIC1);
        let working_entry = root_entry(&fixtures::BASIC2);

        let diff = compute_diff(ctx.clone(), working_entry, Some(parent_entry), None);
        let expected_diff = vec![
            deleted("dir1/file-to-dir"),
            // dir1/file-to-dir/foobar *is* a result, because it has changed and its parent is
            // deleted.
            changed("dir1/file-to-dir/foobar", FileType::Symlink, THREES_FNID),
            changed("dir1/foo", FileType::Regular, THREES_FNID),
            deleted("dir2/bar"),
            changed("dir2/dir-to-file", FileType::Executable, DS_FNID),
            // dir2/dir-to-file/foo is *not* a result, because its parent is marked changed
            changed_reused_id("dir2/only-file-type", FileType::Executable, ONES_FNID),
            changed("dir2/quux", FileType::Symlink, FOURS_FNID),
        ];

        assert_eq!(diff, expected_diff);

        // Test out multiple parents with the same hashes.
        let parent1 = root_entry(&fixtures::BASIC1);
        let parent2 = root_entry(&fixtures::BASIC1);
        let working_entry = root_entry(&fixtures::BASIC2);

        let diff = compute_diff(ctx.clone(), working_entry, Some(parent1), Some(parent2));
        assert_eq!(diff, expected_diff);
    })
}

#[test]
fn diff_truncate() {
    tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let parent_entry = root_entry(&fixtures::TRUNCATE1);
        let working_entry = root_entry(&fixtures::TRUNCATE2);

        let diff = bonsai_diff(ctx, working_entry, Some(parent_entry), None);
        let paths = diff.collect().wait().expect("computing diff failed");
        assert_eq!(paths, vec![]);
    })
}

#[test]
fn diff_merge1() {
    tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let parent1 = root_entry(&fixtures::BASIC1);
        let parent2 = root_entry(&fixtures::BASIC2);
        let working_entry = root_entry(&fixtures::BASIC2);

        let diff = compute_diff(ctx.clone(), working_entry, Some(parent1), Some(parent2));

        // Compare this result to expected_diff in diff_basic.
        let expected_diff = vec![
            deleted("dir1/file-to-dir"),
            // dir1/file-to-dir/foobar is *not* a result because p1 doesn't have it and p2 has the
            // same contents.
            //
            // This ID was reused from parent2.
            changed_reused_id("dir1/foo", FileType::Regular, THREES_FNID),
            deleted("dir2/bar"),
            // This ID was reused from parent2.
            changed_reused_id("dir2/dir-to-file", FileType::Executable, DS_FNID),
            changed_reused_id("dir2/only-file-type", FileType::Executable, ONES_FNID),
            // dir2/quux is not a result because it isn't present in p1 and is present in p2, so
            // the version from p2 is implicitly chosen.
        ];
        assert_eq!(diff, expected_diff);
    })
}

fn root_entry(mf: &ManifestFixture) -> Box<dyn HgEntry + Sync> {
    let path_hashes = mf.path_hashes.iter().cloned();
    let dir_hashes = mf.dir_hashes.iter().cloned();
    let mock_manifest =
        MockManifest::from_path_hashes(path_hashes, dir_hashes).expect("valid manifest");
    let mut entry = MockEntry::from_manifest(RepoPath::RootPath, mock_manifest);
    entry.set_hash(mf.root_hash);
    entry.boxed()
}

fn compute_diff(
    ctx: CoreContext,
    working_entry: Box<dyn HgEntry + Sync>,
    p1_entry: Option<Box<dyn HgEntry + Sync>>,
    p2_entry: Option<Box<dyn HgEntry + Sync>>,
) -> Vec<BonsaiDiffResult> {
    let diff_stream = bonsai_diff(ctx, working_entry, p1_entry, p2_entry);
    let mut paths = diff_stream.collect().wait().expect("computing diff failed");
    paths.sort_unstable();

    check_pcf(paths.iter().map(|diff_result| match diff_result {
        BonsaiDiffResult::Changed(path, ..) | BonsaiDiffResult::ChangedReusedId(path, ..) => {
            (path, true)
        }
        BonsaiDiffResult::Deleted(path) => (path, false),
    }))
    .expect("paths must be path-conflict-free");

    // TODO: check that the result is path-conflict-free
    paths
}

fn changed(path: impl AsRef<[u8]>, ft: FileType, hash: HgFileNodeId) -> BonsaiDiffResult {
    let path = MPath::new(path).expect("valid path");
    BonsaiDiffResult::Changed(path, ft, hash)
}

fn changed_reused_id(path: impl AsRef<[u8]>, ft: FileType, hash: HgFileNodeId) -> BonsaiDiffResult {
    let path = MPath::new(path).expect("valid path");
    BonsaiDiffResult::ChangedReusedId(path, ft, hash)
}

fn deleted(path: impl AsRef<[u8]>) -> BonsaiDiffResult {
    let path = MPath::new(path).expect("valid path");
    BonsaiDiffResult::Deleted(path)
}

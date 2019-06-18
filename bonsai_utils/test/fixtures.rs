// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use mercurial_types::HgNodeHash;
use mononoke_types::FileType;

use mercurial_types_mocks::nodehash::*;

#[derive(Clone, Debug)]
pub struct ManifestFixture {
    pub root_hash: HgNodeHash,
    pub path_hashes: &'static [(&'static str, (FileType, &'static str, HgNodeHash))],
    pub dir_hashes: &'static [(&'static str, HgNodeHash)],
}

// The file contents ("to-change" etc) indicate what's happening to each file.
// It doesn't really matter that we're reusing hashes across different entries --
// what's important is what the hashes are for the same file or directory.

pub const BASIC1: ManifestFixture = ManifestFixture {
    root_hash: AS_HASH,
    path_hashes: &[
        ("dir1/foo", (FileType::Regular, "to-change", ONES_HASH)),
        (
            "dir1/file-to-dir",
            (FileType::Symlink, "file to dir, file", ONES_HASH),
        ),
        ("dir1/fixed", (FileType::Regular, "fixed", NINES_HASH)),
        (
            "dir2/dir-to-file/foo",
            (FileType::Regular, "dir to file, directory", CS_HASH),
        ),
        ("dir2/bar", (FileType::Executable, "to-remove", TWOS_HASH)),
        (
            "dir2/only-file-type",
            (FileType::Regular, "only file type changes", ONES_HASH),
        ),
    ],
    dir_hashes: &[
        ("dir1", FIVES_HASH),
        ("dir2", FIVES_HASH),
        ("dir2/dir-to-file", FIVES_HASH),
    ],
};

pub const BASIC2: ManifestFixture = ManifestFixture {
    root_hash: BS_HASH,
    path_hashes: &[
        ("dir1/foo", (FileType::Regular, "changed", THREES_HASH)),
        (
            "dir1/file-to-dir/foobar",
            (FileType::Symlink, "file to dir, dir", THREES_HASH),
        ),
        ("dir1/fixed", (FileType::Regular, "fixed", NINES_HASH)),
        (
            "dir2/dir-to-file",
            (FileType::Executable, "dir to file, file", DS_HASH),
        ),
        ("dir2/quux", (FileType::Symlink, "added", FOURS_HASH)),
        (
            "dir2/only-file-type",
            (FileType::Executable, "only file type changes", ONES_HASH),
        ),
    ],
    dir_hashes: &[
        ("dir1", SIXES_HASH),
        ("dir2", SIXES_HASH),
        // For this directory, check that a file hash matching a directory hash
        // doesn't cause the walk to be terminated.
        ("dir1/file-to-dir", ONES_HASH),
    ],
};

// Ensure that searches get truncated whenever hashes match.
pub const TRUNCATE1: ManifestFixture = ManifestFixture {
    root_hash: AS_HASH,
    path_hashes: &[
        (
            "dir1/foo",
            (FileType::Regular, "foo in TRUNCATE1", ONES_HASH),
        ),
        (
            "dir2/bar",
            (FileType::Regular, "bar in TRUNCATE1", TWOS_HASH),
        ),
    ],
    dir_hashes: &[("dir1", THREES_HASH), ("dir2", FOURS_HASH)],
};

pub const TRUNCATE2: ManifestFixture = ManifestFixture {
    root_hash: BS_HASH,
    path_hashes: &[
        // dir1/foo here has the same hash as dir1/foo in TRUNCATE1 -- so it should *not* be
        // returned as a result even if the contents are different.
        (
            "dir1/foo",
            (FileType::Regular, "foo in TRUNCATE2", ONES_HASH),
        ),
        (
            "dir2/bar",
            (FileType::Regular, "bar in TRUNCATE2", FIVES_HASH),
        ),
    ],
    dir_hashes: &[
        ("dir1", SIXES_HASH),
        // dir2 here has the same hash as dir2 in TRUNCATE1, so the search shouldn't recurse and
        // figure out that dir2/bar is different.
        ("dir2", FOURS_HASH),
    ],
};

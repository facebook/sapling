/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

// Filemodes as per:
// https://github.com/libgit2/libgit2/blob/68cfb580e19c419992ba0b0a299e5fd6dc60ed99/include/git2/types.h#L210-L217
pub const GIT_FILEMODE_UNREADABLE: i32 = 0o000000;
pub const GIT_FILEMODE_TREE: i32 = 0o040000;
pub const GIT_FILEMODE_BLOB: i32 = 0o100644;
pub const GIT_FILEMODE_BLOB_EXECUTABLE: i32 = 0o100755;
pub const GIT_FILEMODE_LINK: i32 = 0o120000;
pub const GIT_FILEMODE_COMMIT: i32 = 0o160000;

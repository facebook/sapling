/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Git compatibility
//!
//! This crate provides misc features interacting with `git` so high
//! level logic does not have to couple with Git directly.
//!
//! The interactions with `git` is mostly through the `git` CLI, which might
//! include the latest features from the Git upstream that does not exist in
//! other Git implementations.
//!
//! This crate does not provide high level abstractions like the working copy,
//! the storage, etc. It is at a lower level, similar to `edenfs-client`.
//!
//! For now, this crate intentionally avoids depending on libgit2 to avoid
//! link issues with edenfs.

// There are 2 modes to support Git:
// 1. Compatiblity at the exchange protocol layer.
//    Dot directory: `.sl/`.
//    Incompatible with `git` commands.
//    Possible to integrate with most scalability features including edenfs.
// 2. Computability at the `.dot/` layer.
//    Repo dot directory: `.git/`.
//    Compatible with `git` commands.
//    Scalability is mainly limited by what Git provides.
//    Limited scalability features.
//
// Implementation wise the above 2 modes have some overlap:
// - Working copy (status, checkout):
//   1 uses in-house workingcopy (eden, watchman, vanilla)
//   2 uses Git index (status, git-update-index)
// - Storage:
//   1 has potential to use in-house (lazy) storage, although
//     right now it's also the Git-defined storage.
//   2 has to use Git-defined storage (loose file, pack files).
// - Commit graph:
//   Both 1 and 2 sync Git graph to segmented changelog.
//   1 has potential to support "lazytext" changelog.
//   2 has to use the non-lazy graph, and probably wants to
//     filter out refs to "sync" (similar to selective pull).
// - Exchange:
//   Both 1 and 2 shell out to `git fetch` and `git push`
//   commands for now.

/// Utilities about repo initialization.
pub mod init;

/// Run git commands.
pub mod rungit;

/// Work with git references.
mod refs;

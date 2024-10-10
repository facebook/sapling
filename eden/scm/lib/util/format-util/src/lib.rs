/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Utilities interacting with store serialization formats (git or hg).

mod git_sha1;
mod hg_filelog;

pub use git_sha1::git_sha1_deserialize;
pub use git_sha1::git_sha1_serialize;
pub use hg_filelog::parse_copy_from_hg_file_metadata;
pub use hg_filelog::split_hg_file_metadata;
pub use hg_filelog::strip_hg_file_metadata;

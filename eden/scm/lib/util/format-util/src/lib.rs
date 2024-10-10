/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Utilities interacting with store serialization formats (git or hg).

mod hg_filelog;

pub use hg_filelog::parse_copy_from_hg_file_metadata;
pub use hg_filelog::split_hg_file_metadata;
pub use hg_filelog::strip_hg_file_metadata;

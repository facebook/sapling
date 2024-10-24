/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Utilities interacting with store serialization formats (git or hg).

use anyhow::Result;
use minibytes::Text;
use types::Id20;

mod byte_count;
mod commit_fields;
mod git_commit;
mod git_commit_fields;
mod git_sha1;
mod hg_commit;
mod hg_commit_fields;
mod hg_filelog;
mod hg_sha1;
mod sha1_digest;
mod utils;

pub(crate) use byte_count::ByteCount;
pub use commit_fields::CommitFields;
pub use git_commit::git_commit_text_to_root_tree_id;
pub use git_commit_fields::GitCommitFields;
pub use git_commit_fields::GitCommitLazyFields;
pub use git_sha1::git_sha1_deserialize;
pub use git_sha1::git_sha1_digest;
pub use git_sha1::git_sha1_serialize;
pub use git_sha1::git_sha1_serialize_write;
pub use hg_commit::hg_commit_text_to_root_tree_id;
pub use hg_commit_fields::HgCommitFields;
pub use hg_commit_fields::HgCommitLazyFields;
pub use hg_filelog::parse_copy_from_hg_file_metadata;
pub use hg_filelog::split_hg_file_metadata;
pub use hg_filelog::strip_file_metadata;
pub use hg_sha1::hg_sha1_deserialize;
pub use hg_sha1::hg_sha1_digest;
pub use hg_sha1::hg_sha1_serialize;
pub use hg_sha1::hg_sha1_serialize_write;
pub use hgtime::HgTime;
pub(crate) use sha1_digest::Sha1Write;
use storemodel::SerializationFormat;
pub(crate) use utils::normalize_email_user;

pub fn commit_text_to_root_tree_id(text: &[u8], format: SerializationFormat) -> Result<Id20> {
    match format {
        SerializationFormat::Hg => hg_commit_text_to_root_tree_id(text),
        SerializationFormat::Git => git_commit_text_to_root_tree_id(text),
    }
}

pub fn commit_text_to_fields(text: Text, format: SerializationFormat) -> Box<dyn CommitFields> {
    match format {
        SerializationFormat::Hg => Box::new(HgCommitLazyFields::new(text)),
        SerializationFormat::Git => Box::new(GitCommitLazyFields::new(text)),
    }
}

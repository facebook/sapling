/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(error_generic_member_access)]
#![feature(iterator_try_reduce)]

pub mod mode;

mod thrift {
    pub use git_types_thrift::*;
}

mod blob;
mod commit;
mod delta_manifest_ops;
mod delta_manifest_v2;
mod derive_commit;
mod derive_delta_manifest_v2;
mod derive_tree;
mod errors;
pub mod git_lfs;
mod manifest;
mod object;
mod store;
mod tree;

pub use delta_manifest_v2::ObjectKind as DeltaObjectKind;
pub use object::ObjectContent;
pub use object::ObjectKind;

pub use crate::blob::BlobHandle;
pub use crate::commit::MappedGitCommitId;
pub use crate::delta_manifest_ops::fetch_git_delta_manifest;
pub use crate::delta_manifest_ops::GitDeltaManifestEntryOps;
pub use crate::delta_manifest_ops::GitDeltaManifestOps;
pub use crate::delta_manifest_ops::ObjectDeltaOps;
pub use crate::delta_manifest_v2::GDMV2Entry;
pub use crate::delta_manifest_v2::GDMV2ObjectEntry;
pub use crate::delta_manifest_v2::GitDeltaManifestV2;
pub use crate::derive_delta_manifest_v2::RootGitDeltaManifestV2Id;
pub use crate::errors::GitError;
pub use crate::store::fetch_git_object;
pub use crate::store::fetch_git_object_bytes;
pub use crate::store::fetch_non_blob_git_object;
pub use crate::store::fetch_non_blob_git_object_bytes;
pub use crate::store::fetch_packfile_base_item;
pub use crate::store::fetch_packfile_base_item_if_exists;
pub use crate::store::upload_non_blob_git_object;
pub use crate::store::upload_packfile_base_item;
pub use crate::store::GitIdentifier;
pub use crate::store::HeaderState;
pub use crate::tree::Tree;
pub use crate::tree::TreeBuilder;
pub use crate::tree::TreeHandle;
pub use crate::tree::TreeMember;
pub use crate::tree::Treeish;

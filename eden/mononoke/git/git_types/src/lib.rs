/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(error_generic_member_access)]
#![feature(provide_any)]

pub mod mode;

mod thrift {
    pub use git_types_thrift::*;
}

mod blob;
mod derive_tree;
mod errors;
mod manifest;
mod nodehash;
mod object;
mod store;
mod tree;

pub use object::ObjectKind;

pub use crate::blob::BlobHandle;
pub use crate::errors::GitError;
pub use crate::nodehash::GitSha1Prefix;
pub use crate::nodehash::GitSha1sResolvedFromPrefix;
pub use crate::store::upload_git_object;
pub use crate::tree::Tree;
pub use crate::tree::TreeBuilder;
pub use crate::tree::TreeHandle;
pub use crate::tree::TreeMember;
pub use crate::tree::Treeish;

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

pub use crate::bonsai::{bonsai_diff, BonsaiDiffFileChange};
pub use crate::derive::{derive_manifest, derive_manifest_with_io_sender, LeafInfo, TreeInfo};
pub use crate::implicit_deletes::get_implicit_deletes;
pub use crate::ops::{
    find_intersection_of_diffs, find_intersection_of_diffs_and_parents, Diff, ManifestOps,
};
pub use crate::ordered_ops::ManifestOrderedOps;
pub use crate::select::PathOrPrefix;
pub use crate::types::{Entry, Manifest, OrderedManifest, PathTree, Traced};
pub use blobstore::StoreLoadable;
pub use derive_batch::{derive_manifests_for_simple_stack_of_commits, ManifestChanges};

mod bonsai;
mod derive;
mod derive_batch;
mod implicit_deletes;
mod ops;
mod ordered_ops;
mod select;
mod types;

#[cfg(test)]
mod tests;

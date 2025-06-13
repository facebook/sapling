/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use quickcheck_arbitrary_derive::Arbitrary;
use thrift_convert::ThriftConvert;

use crate::ChangesetId;
use crate::path::MPath;
use crate::thrift;

#[derive(ThriftConvert, Arbitrary, Debug, Clone, Eq, PartialEq, Hash)]
#[thrift(thrift::bonsai::SubtreeChange)]
pub enum SubtreeChange {
    /// Copy a subtree from another commit and path.  The copy is shallow, so
    /// the Mercurial trees are reused where there are no additional changes.
    #[thrift(thrift::bonsai::SubtreeCopy)]
    SubtreeCopy(SubtreeCopy),
    /// Copy the history of a subtree from another commit and path.  The copy is
    /// deep, so new Mercurial trees are generated for all files.
    #[thrift(thrift::bonsai::SubtreeDeepCopy)]
    SubtreeDeepCopy(SubtreeDeepCopy),
    /// Merge the history of a subtree with another commit and path.
    #[thrift(thrift::bonsai::SubtreeMerge)]
    SubtreeMerge(SubtreeMerge),
    /// Import history from an external repository.
    #[thrift(thrift::bonsai::SubtreeImport)]
    SubtreeImport(SubtreeImport),
}

impl SubtreeChange {
    pub fn copy(from_path: MPath, from_cs_id: ChangesetId) -> Self {
        Self::SubtreeCopy(SubtreeCopy {
            from_path,
            from_cs_id,
        })
    }

    pub fn deep_copy(from_path: MPath, from_cs_id: ChangesetId) -> Self {
        Self::SubtreeDeepCopy(SubtreeDeepCopy {
            from_path,
            from_cs_id,
        })
    }

    pub fn merge(from_path: MPath, from_cs_id: ChangesetId) -> Self {
        Self::SubtreeMerge(SubtreeMerge {
            from_path,
            from_cs_id,
        })
    }

    pub fn import(from_path: MPath, from_commit: String, from_repo_url: String) -> Self {
        Self::SubtreeImport(SubtreeImport {
            from_path,
            from_commit,
            from_repo_url,
        })
    }

    pub fn source(&self) -> Option<ChangesetId> {
        match self {
            Self::SubtreeCopy(copy) => Some(copy.from_cs_id),
            Self::SubtreeDeepCopy(copy) => Some(copy.from_cs_id),
            Self::SubtreeMerge(merge) => Some(merge.from_cs_id),
            Self::SubtreeImport(_) => None,
        }
    }

    /// Source of this subtree change, for all types that originate within this repo.
    pub fn change_source(&self) -> Option<(ChangesetId, &MPath)> {
        match self {
            Self::SubtreeCopy(copy) => Some((copy.from_cs_id, &copy.from_path)),
            Self::SubtreeDeepCopy(copy) => Some((copy.from_cs_id, &copy.from_path)),
            Self::SubtreeMerge(merge) => Some((merge.from_cs_id, &merge.from_path)),
            Self::SubtreeImport(_) => None,
        }
    }

    /// Source of this subtree change, for copy operations only.
    pub fn copy_source(&self) -> Option<(ChangesetId, &MPath)> {
        match self {
            Self::SubtreeCopy(copy) => Some((copy.from_cs_id, &copy.from_path)),
            _ => None,
        }
    }

    /// Source of this subtree change, for copy or deepcopy operations only.
    pub fn copy_or_deep_copy_source(&self) -> Option<(ChangesetId, &MPath)> {
        match self {
            Self::SubtreeCopy(copy) => Some((copy.from_cs_id, &copy.from_path)),
            Self::SubtreeDeepCopy(copy) => Some((copy.from_cs_id, &copy.from_path)),
            _ => None,
        }
    }

    /// Returns true if this subtree change has an altering affect on the
    /// manifest.
    pub fn alters_manifest(&self) -> bool {
        match self {
            Self::SubtreeCopy(_) => true,
            _ => false,
        }
    }

    pub fn replace_source_changeset_id(&mut self, new_cs_id: ChangesetId) {
        match self {
            Self::SubtreeCopy(copy) => copy.from_cs_id = new_cs_id,
            Self::SubtreeDeepCopy(copy) => copy.from_cs_id = new_cs_id,
            Self::SubtreeMerge(merge) => merge.from_cs_id = new_cs_id,
            Self::SubtreeImport(_) => {}
        }
    }
}

#[derive(ThriftConvert, Arbitrary, Debug, Clone, Eq, PartialEq, Hash)]
#[thrift(thrift::bonsai::SubtreeCopy)]
pub struct SubtreeCopy {
    pub from_path: MPath,
    pub from_cs_id: ChangesetId,
}

#[derive(ThriftConvert, Arbitrary, Debug, Clone, Eq, PartialEq, Hash)]
#[thrift(thrift::bonsai::SubtreeDeepCopy)]
pub struct SubtreeDeepCopy {
    pub from_path: MPath,
    pub from_cs_id: ChangesetId,
}

#[derive(ThriftConvert, Arbitrary, Debug, Clone, Eq, PartialEq, Hash)]
#[thrift(thrift::bonsai::SubtreeMerge)]
pub struct SubtreeMerge {
    pub from_path: MPath,
    pub from_cs_id: ChangesetId,
}

#[derive(ThriftConvert, Arbitrary, Debug, Clone, Eq, PartialEq, Hash)]
#[thrift(thrift::bonsai::SubtreeImport)]
pub struct SubtreeImport {
    pub from_path: MPath,
    pub from_commit: String,
    pub from_repo_url: String,
}

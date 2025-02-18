/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use quickcheck_arbitrary_derive::Arbitrary;
use thrift_convert::ThriftConvert;

use crate::path::MPath;
use crate::thrift;
use crate::ChangesetId;

#[derive(ThriftConvert, Arbitrary, Debug, Clone, Eq, PartialEq, Hash)]
#[thrift(thrift::bonsai::SubtreeChange)]
pub enum SubtreeChange {
    /// Copy a subtree from another commit and path.  The copy is shallow, so
    /// the Mercurial trees are re-used where there are no additional changes.
    #[thrift(thrift::bonsai::SubtreeCopy)]
    SubtreeCopy(SubtreeCopy),
    /// Copy the history of a subtree from another commit and path.  The copy is
    /// deep, so new Mercurial trees are generated for all files.
    #[thrift(thrift::bonsai::SubtreeDeepCopy)]
    SubtreeDeepCopy(SubtreeDeepCopy),
    /// Merge the history of a subtree with another commit and path.
    #[thrift(thrift::bonsai::SubtreeMerge)]
    SubtreeMerge(SubtreeMerge),
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

    /// Source of this subtree change, for all types that originate within this repo.
    pub fn change_source(&self) -> Option<(ChangesetId, &MPath)> {
        match self {
            Self::SubtreeCopy(copy) => Some((copy.from_cs_id, &copy.from_path)),
            Self::SubtreeDeepCopy(copy) => Some((copy.from_cs_id, &copy.from_path)),
            Self::SubtreeMerge(merge) => Some((merge.from_cs_id, &merge.from_path)),
        }
    }

    /// Source of this subtree change, for copy operations only.
    pub fn copy_source(&self) -> Option<(ChangesetId, &MPath)> {
        match self {
            Self::SubtreeCopy(copy) => Some((copy.from_cs_id, &copy.from_path)),
            _ => None,
        }
    }

    pub fn replace_source_changeset_id(&mut self, new_cs_id: ChangesetId) {
        match self {
            Self::SubtreeCopy(copy) => copy.from_cs_id = new_cs_id,
            Self::SubtreeDeepCopy(copy) => copy.from_cs_id = new_cs_id,
            Self::SubtreeMerge(merge) => merge.from_cs_id = new_cs_id,
        }
    }
}

#[derive(ThriftConvert, Arbitrary, Debug, Clone, Eq, PartialEq, Hash)]
#[thrift(thrift::bonsai::SubtreeCopy)]
pub struct SubtreeCopy {
    from_path: MPath,
    from_cs_id: ChangesetId,
}

#[derive(ThriftConvert, Arbitrary, Debug, Clone, Eq, PartialEq, Hash)]
#[thrift(thrift::bonsai::SubtreeDeepCopy)]
pub struct SubtreeDeepCopy {
    from_path: MPath,
    from_cs_id: ChangesetId,
}

#[derive(ThriftConvert, Arbitrary, Debug, Clone, Eq, PartialEq, Hash)]
#[thrift(thrift::bonsai::SubtreeMerge)]
pub struct SubtreeMerge {
    from_path: MPath,
    from_cs_id: ChangesetId,
}

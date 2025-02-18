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

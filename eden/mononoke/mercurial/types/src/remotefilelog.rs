/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use edenapi_types::WireHistoryEntry;
use mononoke_types::NonRootMPath;
use types::Parents;
use types::RepoPathBuf as ClientRepoPathBuf;

use crate::blobnode::HgParents;
use crate::nodehash::HgChangesetId;
use crate::nodehash::HgFileNodeId;
use crate::nodehash::NULL_HASH;

/// Mercurial revlogs and remotefilelog pack files have different formats of storing parents
/// if a file was copied or moved. This function converts from mercurial revlog format to
/// remotefilelog format
pub fn convert_parents_to_remotefilelog_format<'a>(
    parents: &HgParents,
    copyfrom: Option<&'a (NonRootMPath, HgFileNodeId)>,
) -> (HgFileNodeId, HgFileNodeId, Option<&'a NonRootMPath>) {
    let (p1, p2) = match parents {
        HgParents::None => (NULL_HASH, NULL_HASH),
        HgParents::One(p) => (p.clone(), NULL_HASH),
        HgParents::Two(p1, p2) => (p1.clone(), p2.clone()),
    };

    if let Some((copied_from, copied_rev)) = copyfrom {
        // Mercurial has a complicated copy/renames logic.
        // If (path1, filenode1) is copied/renamed from (path2, filenode2),
        // filenode1's p1 is set to filenode2, and copy_from path is set to path2
        // filenode1's p2 is null for non-merge commits. It might be non-null for merges.
        (*copied_rev, HgFileNodeId::new(p1), Some(copied_from))
    } else {
        (HgFileNodeId::new(p1), HgFileNodeId::new(p2), None)
    }
}

/// Represents a file history entry in Mercurial's loose file format.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct HgFileHistoryEntry {
    node: HgFileNodeId,
    parents: HgParents,
    linknode: HgChangesetId,
    copyfrom: Option<(NonRootMPath, HgFileNodeId)>,
}

impl HgFileHistoryEntry {
    pub fn new(
        node: HgFileNodeId,
        parents: HgParents,
        linknode: HgChangesetId,
        copyfrom: Option<(NonRootMPath, HgFileNodeId)>,
    ) -> Self {
        Self {
            node,
            parents,
            linknode,
            copyfrom,
        }
    }

    pub fn filenode(&self) -> &HgFileNodeId {
        &self.node
    }

    pub fn parents(&self) -> &HgParents {
        &self.parents
    }

    pub fn linknode(&self) -> &HgChangesetId {
        &self.linknode
    }

    pub fn copyfrom(&self) -> &Option<(NonRootMPath, HgFileNodeId)> {
        &self.copyfrom
    }
}

impl TryFrom<HgFileHistoryEntry> for WireHistoryEntry {
    type Error = Error;
    /// Convert from a representation of a history entry using Mononoke's types to
    /// a representation that uses the Mercurial client's types.
    fn try_from(entry: HgFileHistoryEntry) -> Result<Self> {
        let node = entry.node.into_nodehash().into();
        let linknode = entry.linknode.into_nodehash().into();

        let (parents, copyfrom) = match entry.copyfrom {
            Some((copypath, copyrev)) => {
                let copypath = ClientRepoPathBuf::from_utf8(copypath.to_vec())?;
                let copyrev = copyrev.into_nodehash().into();
                let (p1, _) = Parents::from(entry.parents).into_nodes();
                let parents = Parents::new(copyrev, p1);
                (parents, Some(copypath))
            }
            None => (entry.parents.into(), None),
        };

        Ok(Self {
            node,
            parents,
            linknode,
            copyfrom,
        })
    }
}

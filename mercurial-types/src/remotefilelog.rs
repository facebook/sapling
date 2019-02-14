// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io::Write;

use failure::Result;

use mononoke_types::MPath;

use blobnode::HgParents;
use nodehash::{HgChangesetId, HgFileNodeId, NULL_HASH};

/// Represents a file history entry in Mercurial's loose file format.
pub struct HgFileHistoryEntry {
    node: HgFileNodeId,
    parents: HgParents,
    linknode: HgChangesetId,
    copyfrom: Option<(MPath, HgFileNodeId)>,
}

impl HgFileHistoryEntry {
    pub fn new(
        node: HgFileNodeId,
        parents: HgParents,
        linknode: HgChangesetId,
        copyfrom: Option<(MPath, HgFileNodeId)>,
    ) -> Self {
        Self {
            node,
            parents,
            linknode,
            copyfrom,
        }
    }

    /// Serialize this entry into Mercurial's loose file format and write
    /// the resulting bytes to the given writer (most likely representing
    /// partially written loose file contents).
    pub fn write_to_loose_file<W: Write>(&self, writer: &mut W) -> Result<()> {
        let (p1, p2) = match self.parents {
            HgParents::None => (NULL_HASH, NULL_HASH),
            HgParents::One(p) => (p, NULL_HASH),
            HgParents::Two(p1, p2) => (p1, p2),
        };

        let (p1, p2, copied_from) = if let Some((ref copied_from, copied_rev)) = self.copyfrom {
            // Mercurial has a complicated copy/renames logic.
            // If (path1, filenode1) is copied/renamed from (path2, filenode2),
            // filenode1's p1 is set to filenode2, and copy_from path is set to path2
            // filenode1's p2 is null for non-merge commits. It might be non-null for merges.
            (copied_rev.into_nodehash(), p1, Some(copied_from))
        } else {
            (p1, p2, None)
        };

        writer.write_all(self.node.clone().into_nodehash().as_bytes())?;
        writer.write_all(p1.as_bytes())?;
        writer.write_all(p2.as_bytes())?;
        writer.write_all(self.linknode.clone().into_nodehash().as_bytes())?;
        if let Some(copied_from) = copied_from {
            writer.write_all(&copied_from.to_vec())?;
        }

        Ok(write!(writer, "\0")?)
    }
}

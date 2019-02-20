// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io::Write;

use failure::Result;

use mononoke_types::MPath;
use types::{api, Key, NodeInfo};

use crate::blobnode::HgParents;
use crate::nodehash::{HgChangesetId, HgFileNodeId, NULL_HASH};

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

    /// Convert this history entry to a format compatible with Mercurial's Rust code,
    /// using types defined by Mercurial's `types` and `revisionstore` crates. These
    /// types are similar to the ones defined in `mercurial-types`, but we want to
    /// use the exact types Mercurial defines to make data transfer between Mononoke
    /// and Mercurial seamless.
    ///
    /// Note that we can't use the `From` trait here because the `HgFileHistoryEntry`
    /// does not actually contain all of the information necessary to construct a
    /// `types::api::HistoryEntry`. In particular, the path of the file the entry
    /// refers to is not present, and therefore must be  provided by the caller.
    pub fn into_api_history_entry(self, path: &MPath) -> api::HistoryEntry {
        let path = path.to_vec();

        let node = self.node.into_nodehash().into();
        let linknode = self.linknode.into_nodehash().into();

        let parents = match self.parents {
            HgParents::None => Default::default(),
            HgParents::One(p1) => {
                // If this file was copied, use the previous path in the p1 key.
                // Otherwise, just use the filenode's current path.
                // See the implementation of `revisionstore::HistoryPack::read_node_info`
                // for the original client-side implementation of this logic.
                let path = self
                    .copyfrom
                    .map(|(p, _)| p.to_vec())
                    .unwrap_or_else(|| path.clone());
                let p1 = Key::new(path, p1.into());
                [p1, Key::default()]
            }
            HgParents::Two(p1, p2) => {
                let p1 = Key::new(path.clone(), p1.into());
                let p2 = Key::new(path.clone(), p2.into());
                [p1, p2]
            }
        };

        let key = Key::new(path, node);
        let nodeinfo = NodeInfo { parents, linknode };
        api::HistoryEntry { key, nodeinfo }
    }
}

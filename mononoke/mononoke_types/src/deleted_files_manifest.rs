/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Result;
use failure_ext::chain::ChainExt;

use crate::blob::{Blob, BlobstoreValue, DeletedManifestBlob};
use crate::errors::ErrorKind;
use crate::path::MPathElement;
use crate::thrift;
use crate::typed_hash::{ChangesetId, DeletedManifestContext, DeletedManifestId};

use fbthrift::compact_protocol;
use std::collections::BTreeMap;

/// Deleted Files Manifest is a data structure that tracks deleted files and commits where they
/// were deleted. This manifest was designed in addition to Unodes to make following file history
/// across deletions possible.
///
/// Both directories and files are represented by the same data structure, which consists of:
/// * optional<linknode>: if set, a changeset where this path was deleted
/// * subentries: a map from base name to the deleted files manifest for this path
/// Even though the manifest tracks only deleted paths, it will still have entries for the
/// existing directories where files were deleted. Optional field `linknode` indicates whether the
/// path still exists (not set) or it was deleted.
///
/// Q&A
///
/// Why the manifest has same data structure for files and directories?
///
/// Deleted files manifest doesn't differ files from directories, because any file path can be
/// reincarnated after the deletion as a directory and vice versa. The manifest doesn't need
/// to know whether the path is a directory or a file, the only important information is "if the
/// path was deleted, which changeset did it?"
///
/// Why we don't keep a path_hash even though it provides uniquness of entries?
///
/// The deleted manifest entry doesn't have a path_hash, that means the entries are identical
/// for different files deleted in the same commit. This is fine as soon as we don't care about the
/// the uniqueness, but about the fact that the files were deleted. So directory entry will have
/// links to the same entry for different deleted files.
/// However, if one of such files is recreated as a directory, we anyway create a new entry for it:
/// manifest entries are immutable.
///
/// How we derive deleted files manifest?
///
/// Assuming we have a computed deleted files manifests for all the current commits, for a new
/// changeset:
/// 1. For each deleted file create a new manifest entry with a linknode to the changeset, where
/// file was deleted.
/// 2. For each recreated file remove this file from the manifest.
/// 3. Remove directory manifest if it still exists and don’t have deleted children anymore.
/// 4. Create new manifest nodes recursively.
/// 5. Finalize the conversion by recording the mapping from changeset id to the root deleted
/// files manifest hash.
///
/// Where does point a linknode for a file that was markes as deleted in a merge commit?
///
/// Linknode will point to the merge commit itself, if the file deletion was made in some of the
/// parents and was accepted in merge commit.
///
/// How do we perform tracking file history across deletions?
///
/// Assume we have a commit graph, where:
/// * - file was deleted
/// o - file exists
///
///   o A
///   |
///   * B
///  / \
/// *   * C
/// |   |
/// : D o E
/// |   |
/// o F :
///
/// 1. Check deleted files manifest from node B: the file was deleted with linknode to B.
/// 2. Need to consider all ancestors of B:
///    * check unodes for the parents: if the file exists,
///        traverse the history
///    * if not, check the deleted manifest whether the file
///        existed and was deleted

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct DeletedManifest {
    linknode: Option<ChangesetId>,
    subentries: BTreeMap<MPathElement, DeletedManifestId>,
}

impl DeletedManifest {
    pub fn new(
        linknode: Option<ChangesetId>,
        subentries: BTreeMap<MPathElement, DeletedManifestId>,
    ) -> Self {
        Self {
            linknode,
            subentries,
        }
    }

    pub fn lookup(&self, basename: &MPathElement) -> Option<&DeletedManifestId> {
        self.subentries.get(basename)
    }

    pub fn list(&self) -> impl Iterator<Item = (&MPathElement, &DeletedManifestId)> {
        self.subentries.iter()
    }

    pub fn linknode(&self) -> &Option<ChangesetId> {
        &self.linknode
    }

    pub fn is_deleted(&self) -> bool {
        self.linknode.is_some()
    }

    pub fn get_manifest_id(&self) -> DeletedManifestId {
        *self.clone().into_blob().id()
    }

    pub(crate) fn from_thrift(t: thrift::DeletedManifest) -> Result<DeletedManifest> {
        let linknode = match t.linknode {
            Some(cs_id) => Some(ChangesetId::from_thrift(cs_id)?),
            None => None,
        };
        let subentries = t
            .subentries
            .into_iter()
            .map(|(basename, entry)| {
                let basename = MPathElement::from_thrift(basename)?;
                let entry = DeletedManifestId::from_thrift(entry)?;

                Ok((basename, entry))
            })
            .collect::<Result<_>>()?;
        Ok(DeletedManifest {
            linknode,
            subentries,
        })
    }

    pub(crate) fn into_thrift(self) -> thrift::DeletedManifest {
        let subentries: BTreeMap<_, _> = self
            .subentries
            .into_iter()
            .map(|(basename, entry)| (basename.into_thrift(), entry.into_thrift()))
            .collect();
        thrift::DeletedManifest {
            linknode: self.linknode.map(|linknode| linknode.into_thrift()),
            subentries,
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(bytes)
            .chain_err(ErrorKind::BlobDeserializeError("DeletedManifest".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

impl BlobstoreValue for DeletedManifest {
    type Key = DeletedManifestId;

    fn into_blob(self) -> DeletedManifestBlob {
        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(&thrift);
        let mut context = DeletedManifestContext::new();
        context.update(&data);
        let id = context.finish();
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data().as_ref())
    }
}

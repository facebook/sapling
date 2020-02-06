/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::{HgFileNodeId, HgNodeHash, Type};
use mononoke_types::{ContentId, RepoPath};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Corrupt hg filenode returned: {expected} != {actual}")]
    CorruptHgFileNode {
        expected: HgFileNodeId,
        actual: HgFileNodeId,
    },
    #[error("Content blob missing for id: {0}")]
    ContentBlobMissing(ContentId),
    #[error("Mercurial content missing for node {0} (type {1})")]
    HgContentMissing(HgNodeHash, Type),
    #[error("Error while deserializing file node retrieved from key '{0}'")]
    FileNodeDeserializeFailed(String),
    #[error("Error while deserializing manifest retrieved from key '{0}'")]
    ManifestDeserializeFailed(String),
    #[error("Incorrect LFS file content {0}")]
    IncorrectLfsFileContent(String),
    #[error("Inconsistent node hash for entry: path {0}, provided: {1}, computed: {2}")]
    InconsistentEntryHash(RepoPath, HgNodeHash, HgNodeHash),
}

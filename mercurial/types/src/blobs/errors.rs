// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::{HgFileNodeId, HgNodeHash, Type};
use failure::Fail;
use mononoke_types::ContentId;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Corrupt hg filenode returned: {} != {}", _0, _1)]
    CorruptHgFileNode {
        expected: HgFileNodeId,
        actual: HgFileNodeId,
    },
    #[fail(display = "Content blob missing for id: {}", _0)]
    ContentBlobMissing(ContentId),
    #[fail(display = "Mercurial content missing for node {} (type {})", _0, _1)]
    HgContentMissing(HgNodeHash, Type),
    #[fail(
        display = "Error while deserializing file node retrieved from key '{}'",
        _0
    )]
    FileNodeDeserializeFailed(String),
    #[fail(
        display = "Error while deserializing manifest retrieved from key '{}'",
        _0
    )]
    ManifestDeserializeFailed(String),
    #[fail(display = "Incorrect LFS file content {}", _0)]
    IncorrectLfsFileContent(String),
}

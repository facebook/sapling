/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mercurial_types::HgNodeHash;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("The following hgid is unexpectedly missing in the blobstore: {0}")]
    MissingInBlobstore(HgNodeHash),
    #[error(
        "Content metadata is unexpectedly missing in the blobstore for the following hgid: {0}"
    )]
    ContentMetadataMissingInBlobstore(HgNodeHash),
}

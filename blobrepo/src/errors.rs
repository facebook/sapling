// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::error;

use mercurial_types::{NodeHash, hash};

#[recursion_limit = "1024"]
error_chain! {
    errors {
        Head(err: Box<error::Error + Send + 'static>) {
            description("Head error")
            display("Head error: {}", err)
        }
        Blobstore(err: Box<error::Error + Send + 'static>) {
            description("Blobstore error")
            display("Blobstore error: {}", err)
        }
        ChangesetMissing(nodeid: NodeHash) {
            description("Missing Changeset")
            display("Changeset id {} is missing", nodeid)
        }
        ManifestMissing(nodeid: NodeHash) {
            description("Missing Manifest")
            display("Manifest id {} is missing", nodeid)
        }
        NodeMissing(nodeid: NodeHash) {
            description("Missing Node")
            display("Node id {} is missing", nodeid)
        }
        ContentMissing(nodeid: NodeHash, sha1: hash::Sha1) {
            description("Missing Content")
            display("Content missing nodeid {} sha1 {}", nodeid, sha1)
        }
    }

    links {
        Mercurial(::mercurial::Error, ::mercurial::ErrorKind);
        MercurialTypes(::mercurial_types::Error, ::mercurial_types::ErrorKind);
    }

    foreign_links {
        Bincode(::bincode::Error);
    }
}

// We don't know the concrete type of the Heads or Blobstore errors, and we
// can't currently parameterize Error even if we did, so make do with boxing
// those errors up.
pub fn head_err<E>(err: E) -> Error
where
    E: error::Error + Send + 'static,
{
    ErrorKind::Head(Box::new(err)).into()
}

pub fn blobstore_err<E>(err: E) -> Error
where
    E: error::Error + Send + 'static,
{
    ErrorKind::Blobstore(Box::new(err)).into()
}

// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::error;

use error_chain::ChainedError;

use mercurial_types::{NodeHash, hash};

#[recursion_limit = "1024"]
error_chain! {
    errors {
        Heads {
            description("Heads error")
        }
        Blobstore {
            description("Blobstore error")
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

// The specific Heads implementation we're using can have its own Error type,
// so we can't treat it as a foreign link. Instead, have a local ErrorKind for
// representing Heads errors which is chained onto the underlying error.
pub fn heads_err<E: error::Error + Send + 'static>(err: E) -> Error {
    ChainedError::with_chain(err, ErrorKind::Heads)
}

// Handle Blobstore errors in the same way as Heads.
pub fn blobstore_err<E: error::Error + Send + 'static>(err: E) -> Error {
    ChainedError::with_chain(err, ErrorKind::Blobstore)
}

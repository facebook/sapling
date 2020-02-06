/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

// Utilities to lookup FilenodeIds and avoid recomputation.
//
// The motivation behind this is that given file contents, copy info, and parents,
// the file node ID is deterministic, but computing it requires fetching and
// hashing the body of the file. This implementation implements a lookup through
// the blobstore to find a pre-computed filenode ID.
//
// Doing so is in general a little inefficient (but it's not entirely certain that
// the implementation I'm proposing here is faster -- more on this below), but
// it's particularly problematic large files. Indeed, fetching a multiple-GB file
// to recompute the filenode even if we received it from the client can be fairly
// slow (and use up quite a bit of RAM, though that's something we can mitigate by
// streaming file contents).

use crate::HgFileNodeId;
use anyhow::Error;
use ascii::AsciiString;
use blobstore::Blobstore;
use context::CoreContext;
use futures::Future;
use mononoke_types::{BlobstoreBytes, ContentId, MPath};

#[derive(Debug, Eq, Hash, PartialEq)]
pub struct FileNodeIdPointer(String);

impl FileNodeIdPointer {
    pub fn new(
        content_id: &ContentId,
        copy_from: &Option<(MPath, HgFileNodeId)>,
        p1: &Option<HgFileNodeId>,
        p2: &Option<HgFileNodeId>,
    ) -> Self {
        let p1 = p1
            .as_ref()
            .map(HgFileNodeId::to_hex)
            .unwrap_or(AsciiString::new());

        let p2 = p2
            .as_ref()
            .map(HgFileNodeId::to_hex)
            .unwrap_or(AsciiString::new());

        let (copy_from_path, copy_from_filenode) = copy_from
            .as_ref()
            .map(|(mpath, fnid)| {
                let path_hash = mpath.get_path_hash();
                (path_hash.to_hex(), fnid.to_hex())
            })
            .unwrap_or((AsciiString::new(), AsciiString::new()));

        // Put all the deterministic parts together with a separator that cannot show up in them
        // (those are all hex digests or empty strings), then hash them. We'd like to use those
        // directly as our key, but that won't work in blobstores that limit the length of our keys
        // (e.g. SqlBlob).
        let mut ctx = mononoke_types::hash::Context::new("hgfilenode".as_bytes());

        ctx.update(&content_id);
        ctx.update(b".");
        ctx.update(&p1);
        ctx.update(b".");
        ctx.update(&p2);
        ctx.update(b".");
        ctx.update(&copy_from_path);
        ctx.update(b".");
        ctx.update(&copy_from_filenode);

        let key = format!("filenode_lookup.{}", ctx.finish().to_hex());
        Self(key)
    }
}

pub fn store_filenode_id<B: Blobstore>(
    ctx: CoreContext,
    blobstore: &B,
    key: FileNodeIdPointer,
    filenode_id: &HgFileNodeId,
) -> impl Future<Item = (), Error = Error> {
    let contents = BlobstoreBytes::from_bytes(filenode_id.as_bytes());
    blobstore.put(ctx, key.0, contents)
}

pub fn lookup_filenode_id<B: Blobstore>(
    ctx: CoreContext,
    blobstore: &B,
    key: FileNodeIdPointer,
) -> impl Future<Item = Option<HgFileNodeId>, Error = Error> {
    blobstore.get(ctx.clone(), key.0).map(|maybe_blob| {
        maybe_blob.and_then(|blob| HgFileNodeId::from_bytes(blob.as_bytes()).ok())
    })
}

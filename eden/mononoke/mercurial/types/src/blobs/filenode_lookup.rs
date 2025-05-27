/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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

use std::time::Duration;

use anyhow::Result;
use ascii::AsciiString;
use blobstore::Blobstore;
use bytes::Bytes;
use context::CoreContext;
use futures::Future;
use mononoke_types::BlobstoreBytes;
use mononoke_types::ContentId;
use mononoke_types::NonRootMPath;
use stats::prelude::*;
use tokio::time::timeout;

use crate::HgFileNodeId;

define_stats! {
    prefix = "mononoke.mercurial.filenode_lookup";
    timeout: timeseries(Rate, Sum),
}

#[derive(Debug, Eq, Hash, PartialEq)]
pub struct FileNodeIdPointer(String);

impl FileNodeIdPointer {
    pub fn new(
        content_id: &ContentId,
        copy_from: &Option<(NonRootMPath, HgFileNodeId)>,
        p1: &Option<HgFileNodeId>,
        p2: &Option<HgFileNodeId>,
    ) -> Self {
        let p1 = p1.as_ref().map(HgFileNodeId::to_hex).unwrap_or_default();

        let p2 = p2.as_ref().map(HgFileNodeId::to_hex).unwrap_or_default();

        let (copy_from_path, copy_from_filenode) =
            copy_from
                .as_ref()
                .map_or((AsciiString::new(), AsciiString::new()), |(mpath, fnid)| {
                    let path_hash = mpath.get_path_hash();
                    (path_hash.to_hex(), fnid.to_hex())
                });

        // Put all the deterministic parts together with a separator that cannot show up in them
        // (those are all hex digests or empty strings), then hash them. We'd like to use those
        // directly as our key, but that won't work in blobstores that limit the length of our keys
        // (e.g. SqlBlob).
        let mut ctx = mononoke_types::hash::Context::new("hgfilenode".as_bytes());

        ctx.update(content_id);
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
    pub fn blobstore_key(&self) -> String {
        self.0.clone()
    }
}

pub fn store_filenode_id<'a, B: Blobstore>(
    ctx: &'a CoreContext,
    blobstore: &'a B,
    key: FileNodeIdPointer,
    filenode_id: &HgFileNodeId,
) -> impl Future<Output = Result<()>> + use<'a, B> {
    let contents = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(filenode_id.as_bytes()));
    blobstore.put(ctx, key.0, contents)
}

pub async fn lookup_filenode_id<B: Blobstore>(
    ctx: &CoreContext,
    blobstore: &B,
    key: FileNodeIdPointer,
) -> Result<Option<HgFileNodeId>> {
    let filenode_lookup_timeout_ms: u64 = 1000;
    let fut = blobstore.get(ctx, &key.0);
    let maybe_timed_out = timeout(Duration::from_millis(filenode_lookup_timeout_ms), fut).await;
    let blob = match maybe_timed_out {
        Ok(blob) => blob?,
        Err(_) => {
            STATS::timeout.add_value(1);
            return Ok(None);
        }
    };

    Ok(blob.and_then(|blob| HgFileNodeId::from_bytes(blob.as_raw_bytes()).ok()))
}

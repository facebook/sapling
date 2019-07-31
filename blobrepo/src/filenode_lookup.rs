// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

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

use ascii::AsciiString;
use blobstore::Blobstore;
use context::CoreContext;
use failure_ext::Error;
use futures::Future;
use mercurial_types::HgFileNodeId;
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

#[cfg(test)]
mod test {
    use super::*;
    use failure_ext::{err_msg, Error};
    use mercurial_types::RepoPath;
    use mercurial_types_mocks::nodehash::{FOURS_FNID, ONES_FNID, THREES_FNID, TWOS_FNID};
    use mononoke_types_mocks::contentid::{ONES_CTID, TWOS_CTID};
    use std::collections::HashSet;

    #[test]
    fn test_hashes_are_unique() -> Result<(), Error> {
        let mut h = HashSet::new();

        for content_id in [ONES_CTID, TWOS_CTID].iter() {
            for p1 in [Some(ONES_FNID), Some(TWOS_FNID), None].iter() {
                for p2 in [Some(THREES_FNID), Some(FOURS_FNID), None].iter() {
                    let path1 = RepoPath::file("path")?
                        .into_mpath()
                        .ok_or(err_msg("path1"))?;

                    let path2 = RepoPath::file("path/2")?
                        .into_mpath()
                        .ok_or(err_msg("path2"))?;

                    let path3 = RepoPath::file("path2")?
                        .into_mpath()
                        .ok_or(err_msg("path3"))?;

                    for copy_path in [path1, path2, path3].iter() {
                        for copy_parent in [ONES_FNID, TWOS_FNID, THREES_FNID].iter() {
                            let copy_info = Some((copy_path.clone(), copy_parent.clone()));

                            let ptr = FileNodeIdPointer::new(&content_id, &copy_info, p1, p2);
                            assert!(!h.contains(&ptr), format!("Duplicate entry: {:?}", ptr));
                            h.insert(ptr);

                            if p1 == p2 {
                                continue;
                            }

                            let ptr = FileNodeIdPointer::new(&content_id, &copy_info, p2, p1);
                            assert!(!h.contains(&ptr), format!("Duplicate entry: {:?}", ptr));
                            h.insert(ptr);
                        }
                    }

                    let ptr = FileNodeIdPointer::new(&content_id, &None, p1, p2);
                    assert!(!h.contains(&ptr), format!("Duplicate entry: {:?}", ptr));
                    h.insert(ptr);

                    if p1 == p2 {
                        continue;
                    }

                    let ptr = FileNodeIdPointer::new(&content_id, &None, p2, p1);
                    assert!(!h.contains(&ptr), format!("Duplicate entry: {:?}", ptr));
                    h.insert(ptr);
                }
            }
        }

        Ok(())
    }
}

// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobstore::Blobstore;
use bytes::Bytes;
use failure_ext::{format_err, Error, Result};
use futures::future;
use futures::Future;
use quickcheck::{rand, Arbitrary, StdGen};
use std::collections::HashSet;

use crate::incremental_hash::{
    hash_bytes, ContentIdIncrementalHasher, GitSha1IncrementalHasher, Sha1IncrementalHasher,
    Sha256IncrementalHasher,
};

use crate::*;

use super::failing_blobstore::FailingBlobstore;
use super::request;

/// Fetching through any alias should return the same outcome.
fn check_consistency<B: Blobstore>(
    blobstore: B,
    ctx: CoreContext,
    bytes: &Bytes,
) -> impl Future<Item = bool, Error = Error> {
    // TODO: We shouldn't need a Filestore just to read.
    let filestore = Filestore::new(Arc::new(blobstore));

    let content_id = hash_bytes(ContentIdIncrementalHasher::new(), &bytes);
    let sha1 = hash_bytes(Sha1IncrementalHasher::new(), &bytes);
    let sha256 = hash_bytes(Sha256IncrementalHasher::new(), &bytes);
    let git_sha1 = hash_bytes(GitSha1IncrementalHasher::new(*&bytes), &bytes);

    let futs = vec![
        filestore.fetch(ctx.clone(), &FetchKey::Canonical(content_id)),
        filestore.fetch(ctx.clone(), &FetchKey::Sha1(sha1)),
        filestore.fetch(ctx.clone(), &FetchKey::Sha256(sha256)),
        filestore.fetch(ctx.clone(), &FetchKey::GitSha1(git_sha1)),
    ];

    let futs: Vec<_> = futs.into_iter().map(|f| f.map(|r| r.is_some())).collect();

    future::join_all(futs).and_then(|outcomes| {
        // Either all should exist, or none should exist.
        let h: HashSet<_> = outcomes.iter().collect();
        if h.len() == 1 {
            Ok(*h.into_iter().next().unwrap())
        } else {
            Err(format_err!("Inconsistent fetch results: {:?}", outcomes))
        }
    })
}

fn check_metadata<B: Blobstore>(
    blobstore: B,
    ctx: CoreContext,
    bytes: &Bytes,
) -> impl Future<Item = bool, Error = Error> {
    // TODO: We shouldn't need a Filestore just to read.
    let filestore = Filestore::new(Arc::new(blobstore));

    let content_id = hash_bytes(ContentIdIncrementalHasher::new(), &bytes);

    filestore
        .get_aliases(ctx.clone(), &FetchKey::Canonical(content_id))
        .map(|r| r.is_some())
}

#[test]
fn test_invariants() -> Result<()> {
    // NOTE: We make calls to our Blobstore succeed with 75% probability below. This might seem
    // high, but this actually makes most store() calls fail, since there is a lot that needs to go
    // right for a store() call to succeed (all the chunks need to be saved, then we need to write
    // 3 aliases, and then the content).
    let mut rt = tokio::runtime::Runtime::new()?;
    let mut gen = StdGen::new(rand::thread_rng(), 128);

    let memblob = Arc::new(memblob::LazyMemblob::new());
    let blob = Arc::new(FailingBlobstore::new(memblob.clone(), 0.75, 0.75));
    let filestore = Filestore::with_config(blob, FilestoreConfig { chunk_size: 16 });
    let ctx = CoreContext::test_mock();

    for _ in 0..1000 {
        let bytes = Bytes::from(Vec::arbitrary(&mut gen));
        let req = request(&bytes);

        // Try to store with a broken blobstore. It doesn't matter if we succeed or not.
        let res = rt.block_on(filestore.store(ctx.clone(), &req, stream::once(Ok(bytes.clone()))));
        println!("store: {:?}", res);

        // Try to read with a functional blobstore. All results should be consistent.
        let content_ok = rt.block_on(check_consistency(memblob.clone(), ctx.clone(), &bytes))?;
        println!("content_ok: {:?}", content_ok);

        // If we can read the content metadata, then we should also be able to read a metadata.
        let metadata_ok = rt.block_on(check_metadata(memblob.clone(), ctx.clone(), &bytes))?;
        println!("metadata_ok: {:?}", metadata_ok);
        assert_eq!(content_ok, metadata_ok)
    }

    Ok(())
}

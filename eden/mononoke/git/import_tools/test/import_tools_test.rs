/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::format_err;
use blobstore::KeyedBlobstore;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use filestore::StoreRequest;
use futures::TryStreamExt;
use futures::stream;
use git_types::git_lfs::LfsPointerData;
use gix_hash::ObjectId;
use import_tools::GitImportLfs;
use import_tools::LFS_CONSUMER_RETRY_ATTEMPTS;
use memblob::KeyedMemblob;
use mononoke_macros::mononoke;

/// Seed an in-memory filestore with `content` and return an internal-mode
/// `GitImportLfs` serving it plus a pointer for that object.
async fn seeded_internal_lfs(ctx: &CoreContext, content: &Bytes) -> (GitImportLfs, LfsPointerData) {
    let blobstore: Arc<dyn KeyedBlobstore> = Arc::new(KeyedMemblob::default());
    let metadata = filestore::store(
        &blobstore,
        FilestoreConfig::no_chunking_filestore(),
        ctx,
        &StoreRequest::new(content.len() as u64),
        stream::once(futures::future::ok(content.clone())),
    )
    .await
    .expect("filestore store");
    let pointer = LfsPointerData {
        version: "https://git-lfs.github.com/spec/v1".to_string(),
        sha256: metadata.sha256,
        size: metadata.total_size,
        gitblob: vec![],
        gitid: ObjectId::from_hex(b"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
        is_canonical: true,
    };
    (
        GitImportLfs::new_internal(blobstore, false /* allow_not_found */),
        pointer,
    )
}

#[mononoke::fbinit_test]
async fn with_refetches_when_consumer_fails_then_succeeds(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let content = Bytes::from_static(b"streamed twice");
    let (lfs, pointer) = seeded_internal_lfs(&ctx, &content).await;

    // The first run consumes the stream and then fails the way a connection
    // dropped mid-body does; the second run must receive a fresh, full stream.
    let calls = Arc::new(AtomicUsize::new(0));
    let (got, runs) = lfs
        .with(ctx, pointer, {
            cloned!(calls);
            move |_ctx, _meta, _req, bstream, _fetch_result| {
                cloned!(calls);
                async move {
                    let n = calls.fetch_add(1, Ordering::SeqCst);
                    let chunks: Vec<Bytes> = bstream.try_collect().await?;
                    if n == 0 {
                        return Err(format_err!("peer closed connection"));
                    }
                    Ok((chunks.concat(), n + 1))
                }
            }
        })
        .await
        .expect("second attempt succeeds");
    assert_eq!(got, content);
    assert_eq!(runs, 2, "consumer should have run exactly twice");
}

#[mononoke::fbinit_test]
async fn with_gives_up_after_max_consumer_attempts(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let content = Bytes::from_static(b"never stored");
    let (lfs, pointer) = seeded_internal_lfs(&ctx, &content).await;

    let calls = Arc::new(AtomicUsize::new(0));
    let err = lfs
        .with(ctx, pointer, {
            cloned!(calls);
            move |_ctx, _meta, _req, _bstream, _fetch_result| {
                cloned!(calls);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err::<(), _>(format_err!("error reading a body from connection"))
                }
            }
        })
        .await
        .expect_err("must give up");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        LFS_CONSUMER_RETRY_ATTEMPTS as usize,
        "consumer should run once per attempt"
    );
    let msg = format!("{err:#}");
    assert!(
        msg.contains(&format!(
            "failed after {LFS_CONSUMER_RETRY_ATTEMPTS} attempts"
        )) && msg.contains("error reading a body from connection"),
        "unexpected error: {msg}"
    );
}

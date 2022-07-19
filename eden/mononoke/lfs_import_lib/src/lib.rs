/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context as _;
use anyhow::Error;
use anyhow::Result;
use bytes::Bytes;
use bytes::BytesMut;
use context::CoreContext;
use filestore::Alias;
use filestore::FetchKey;
use filestore::FilestoreConfigRef;
use filestore::StoreRequest;
use futures::future;
use futures::future::TryFutureExt;
use futures::stream::Stream;
use futures::stream::TryStreamExt;
use mercurial_types::blobs::LFSContent;
use mononoke_types::ContentMetadata;
use repo_blobstore::RepoBlobstoreRef;
use slog::info;
use std::process::Stdio;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::Command;
use tokio_util::codec::BytesCodec;
use tokio_util::codec::FramedRead;

fn lfs_stream(
    lfs_helper: &str,
    lfs: &LFSContent,
) -> Result<(Child, impl Stream<Item = Result<Bytes, Error>>)> {
    let mut cmd = Command::new(lfs_helper)
        .arg(format!("{}", lfs.oid().to_hex()))
        .arg(format!("{}", lfs.size()))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("Error starting lfs_helper: {:?}", lfs_helper))?;

    let stdout = cmd
        .stdout
        .take()
        .expect("stdout was piped earlier and is missing here");
    let stdout = BufReader::new(stdout);
    let stream = FramedRead::new(stdout, BytesCodec::new())
        .map_ok(BytesMut::freeze)
        .map_err(Error::from);

    Ok((cmd, stream))
}

async fn do_lfs_upload(
    ctx: &CoreContext,
    repo: &(impl RepoBlobstoreRef + FilestoreConfigRef),
    lfs_helper: &str,
    lfs: &LFSContent,
) -> Result<ContentMetadata, Error> {
    let metadata = filestore::get_metadata(
        repo.repo_blobstore(),
        ctx,
        &FetchKey::Aliased(Alias::Sha256(lfs.oid())),
    )
    .await?;

    if let Some(metadata) = metadata {
        info!(
            ctx.logger(),
            "lfs_upload: reusing blob {:?}", metadata.sha256
        );
        return Ok(metadata);
    }

    info!(ctx.logger(), "lfs_upload: importing blob {:?}", lfs.oid());
    let req = StoreRequest::with_sha256(lfs.size(), lfs.oid());

    let (mut child, stream) = lfs_stream(lfs_helper, lfs)?;

    let upload = filestore::store(
        repo.repo_blobstore(),
        *repo.filestore_config(),
        ctx,
        &req,
        stream,
    );

    // NOTE: We ignore the child exit code here. Since the Filestore validates the object
    // we're uploading by SHA256, that's indeed fine (it doesn't matter if the Child failed
    // if it gave us exactly the content we wanted).
    let (_, meta) = future::try_join(child.wait().map_err(Error::from), upload).await?;

    info!(ctx.logger(), "lfs_upload: imported blob {:?}", meta.sha256);

    Ok(meta)
}

pub async fn lfs_upload(
    ctx: &CoreContext,
    repo: &(impl RepoBlobstoreRef + FilestoreConfigRef),
    lfs_helper: &str,
    lfs: &LFSContent,
) -> Result<ContentMetadata, Error> {
    let max_attempts = 5;
    let mut attempt = 0;

    loop {
        let res = do_lfs_upload(ctx, repo, lfs_helper, lfs).await;

        if res.is_ok() || attempt > max_attempts {
            break res;
        }

        attempt += 1;
    }
}

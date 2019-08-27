// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{Error, Fail, Result};
use filestore::{self, Alias, FetchKey, StoreRequest};
use futures::{
    future::{loop_fn, Loop},
    Future, IntoFuture, Stream,
};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::blobs::LFSContent;
use mononoke_types::ContentMetadata;
use slog::info;
use std::io::BufReader;
use std::process::{Command, Stdio};
use tokio::codec;
use tokio_process::{Child, CommandExt};

fn lfs_stream(
    lfs_helper: &str,
    lfs: &LFSContent,
) -> Result<(Child, impl Stream<Item = Bytes, Error = Error>)> {
    let cmd = Command::new(lfs_helper)
        .arg(format!("{}", lfs.oid().to_hex()))
        .arg(format!("{}", lfs.size()))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn_async();

    cmd.map_err(|e| {
        e.context(format!("While starting lfs_helper: {:?}", lfs_helper))
            .into()
    })
    .map(|mut cmd| {
        let stdout = cmd.stdout().take().expect("stdout was missing");
        let stdout = BufReader::new(stdout);
        let stream = codec::FramedRead::new(stdout, codec::BytesCodec::new())
            .map(|bytes_mut| bytes_mut.freeze())
            .from_err();
        (cmd, stream)
    })
}

fn do_lfs_upload(
    ctx: CoreContext,
    blobrepo: BlobRepo,
    lfs_helper: String,
    lfs: LFSContent,
) -> BoxFuture<ContentMetadata, Error> {
    let blobstore = blobrepo.get_blobstore();

    filestore::get_metadata(
        &blobstore,
        ctx.clone(),
        &FetchKey::Aliased(Alias::Sha256(lfs.oid())),
    )
    .and_then({
        move |metadata| match metadata {
            Some(metadata) => {
                info!(
                    ctx.logger(),
                    "lfs_upload: reusing blob {:?}", metadata.sha256
                );
                Ok(metadata).into_future()
            }
            .left_future(),
            None => {
                info!(ctx.logger(), "lfs_upload: importing blob {:?}", lfs.oid());
                let req = StoreRequest::with_sha256(lfs.size(), lfs.oid());

                lfs_stream(&lfs_helper, &lfs)
                    .into_future()
                    .and_then(move |(child, stream)| {
                        let upload_fut = blobrepo.upload_file(ctx.clone(), &req, stream);

                        // NOTE: We ignore the child exit code here. Since the Filestore validates the object
                        // we're uploading by SHA256, that's indeed fine (it doesn't matter if the Child failed
                        // if it gave us exactly the content we wanted).
                        (upload_fut, child.from_err()).into_future().map({
                            cloned!(ctx);
                            move |(meta, _)| {
                                info!(ctx.logger(), "lfs_upload: imported blob {:?}", meta.sha256);
                                meta
                            }
                        })
                    })
            }
            .right_future(),
        }
    })
    .boxify()
}

pub fn lfs_upload(
    ctx: CoreContext,
    blobrepo: BlobRepo,
    lfs_helper: String,
    lfs: LFSContent,
) -> impl Future<Item = ContentMetadata, Error = Error> {
    let max_attempts = 5;

    loop_fn(0, move |i| {
        do_lfs_upload(
            ctx.clone(),
            blobrepo.clone(),
            lfs_helper.clone(),
            lfs.clone(),
        )
        .then(move |r| {
            let loop_state = if r.is_ok() || i > max_attempts {
                Loop::Break(r)
            } else {
                Loop::Continue(i + 1)
            };
            Ok(loop_state)
        })
    })
    .and_then(|r| r)
}

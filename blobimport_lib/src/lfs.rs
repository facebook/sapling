// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::{BlobRepo, StoreRequest};
use bytes::Bytes;
use context::CoreContext;
use failure::{Error, Result};
use futures::{Future, IntoFuture, Stream};
use mercurial::file::LFSContent;
use mononoke_types::ContentMetadata;
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

    cmd.map_err(|e| e.into()).map(|mut cmd| {
        let stdout = cmd.stdout().take().expect("stdout was missing");
        let stdout = BufReader::new(stdout);
        let stream = codec::FramedRead::new(stdout, codec::BytesCodec::new())
            .map(|bytes_mut| bytes_mut.freeze())
            .from_err();
        (cmd, stream)
    })
}

pub fn lfs_upload(
    ctx: CoreContext,
    blobrepo: BlobRepo,
    lfs_helper: &str,
    lfs: &LFSContent,
) -> impl Future<Item = ContentMetadata, Error = Error> {
    let req = StoreRequest::with_sha256(lfs.size(), lfs.oid());

    lfs_stream(lfs_helper, lfs)
        .into_future()
        .and_then(move |(child, stream)| {
            let upload_fut = blobrepo.upload_file(ctx, &req, stream);

            // NOTE: We ignore the child exit code here. Since the Filestore validates the object
            // we're uploading by SHA256, that's indeed fine (it doesn't matter if the Child failed
            // if it gave us exactly the content we wanted).
            (upload_fut, child.from_err())
                .into_future()
                .map(|(chunk, _)| chunk)
        })
}

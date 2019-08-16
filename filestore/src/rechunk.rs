// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure_ext::{Error, Fail};
use futures::future::IntoFuture;
use futures::Future;
use futures_ext::FutureExt;

use blobstore::{Blobstore, Loadable};
use context::CoreContext;
use mononoke_types::{ContentId, ContentMetadata};

use crate::fetch::stream_file_bytes;
use crate::{store, FilestoreConfig, StoreRequest};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Content not found: {:?}", _0)]
    ContentNotFound(ContentId),
}

/// Fetch a file from the blobstore and reupload it in a chunked form.
/// NOTE: This could actually unchunk a file if the chunk size threshold
/// is increased after the file is written.
pub fn rechunk<B: Blobstore + Clone>(
    blobstore: B,
    config: FilestoreConfig,
    ctx: CoreContext,
    content_id: ContentId,
) -> impl Future<Item = ContentMetadata, Error = Error> {
    content_id
        .load(ctx.clone(), &blobstore)
        .and_then(move |maybe_file_contents| match maybe_file_contents {
            Some(file_contents) => {
                let req = StoreRequest::with_canonical(file_contents.size(), content_id);
                let file_stream = stream_file_bytes(blobstore.clone(), ctx.clone(), file_contents);
                store(&blobstore, &config, ctx, &req, file_stream).left_future()
            }
            None => Err(ErrorKind::ContentNotFound(content_id).into())
                .into_future()
                .right_future(),
        })
}

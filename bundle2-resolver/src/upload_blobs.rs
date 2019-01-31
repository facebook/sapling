// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::sync::Arc;

use futures::Stream;
use futures_ext::{BoxFuture, FutureExt};

use blobrepo::BlobRepo;
use context::CoreContext;
use mercurial_types::HgNodeKey;

use errors::*;

/// Represents data that is Mercurial-encoded and can be uploaded to the blobstore.
pub trait UploadableHgBlob {
    type Value: Send + 'static;

    fn upload(self, ctx: CoreContext, repo: &BlobRepo) -> Result<(HgNodeKey, Self::Value)>;
}

#[derive(PartialEq, Eq)]
pub enum UploadBlobsType {
    IgnoreDuplicates,
    EnsureNoDuplicates,
}
use self::UploadBlobsType::*;

pub fn upload_hg_blobs<S, B>(
    ctx: CoreContext,
    repo: Arc<BlobRepo>,
    blobs: S,
    ubtype: UploadBlobsType,
) -> BoxFuture<HashMap<HgNodeKey, B::Value>, Error>
where
    S: Stream<Item = B, Error = Error> + Send + 'static,
    B: UploadableHgBlob,
{
    blobs
        .fold(HashMap::new(), move |mut map, item| {
            let (key, value) = item.upload(ctx.clone(), &repo)?;
            ensure_msg!(
                map.insert(key.clone(), value).is_none() || ubtype == IgnoreDuplicates,
                "HgBlob {:?} already provided before",
                key
            );
            Ok(map)
        })
        .boxify()
}

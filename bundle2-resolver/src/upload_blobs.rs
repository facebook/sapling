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
use mercurial_types::HgNodeKey;

use errors::*;

/// Represents data that is Mercurial-encoded and can be uploaded to the blobstore.
pub trait UploadableHgBlob {
    type Value: Send + 'static;

    fn upload(self, repo: &BlobRepo) -> Result<(HgNodeKey, Self::Value)>;
}

/// Represents data that is Thrift-encoded and can be uploaded to the blobstore.
pub trait UploadableBlob {
    type Value: Send + 'static;

    fn upload(self, repo: &BlobRepo) -> Result<(HgNodeKey, Self::Value)>;
}

#[derive(PartialEq, Eq)]
pub enum UploadBlobsType {
    IgnoreDuplicates,
    EnsureNoDuplicates,
}
use self::UploadBlobsType::*;

pub fn upload_hg_blobs<S, B>(
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
            let (key, value) = item.upload(&repo)?;
            ensure_msg!(
                map.insert(key.clone(), value).is_none() || ubtype == IgnoreDuplicates,
                "HgBlob {:?} already provided before",
                key
            );
            Ok(map)
        })
        .boxify()
}

/// Upload both Mercurial and Mononoke blobs. (Unfortunately, forking a stream is hard so
/// using separate 'upload Mononoke blobs' method are much harder to use here.)
pub fn upload_blobs<S, B>(
    repo: Arc<BlobRepo>,
    blobs: S,
    ubtype: UploadBlobsType,
) -> BoxFuture<
    (
        HashMap<HgNodeKey, <B as UploadableHgBlob>::Value>,
        HashMap<HgNodeKey, <B as UploadableBlob>::Value>,
    ),
    Error,
>
where
    S: Stream<Item = B, Error = Error> + Send + 'static,
    B: UploadableBlob + UploadableHgBlob + Clone,
{
    blobs
        .fold(
            (HashMap::new(), HashMap::new()),
            move |(mut hg_map, mut thrift_map), item| {
                let (hg_key, hg_value) = UploadableHgBlob::upload(item.clone(), &repo)?;
                let (thrift_key, thrift_value) = UploadableBlob::upload(item, &repo)?;
                ensure_msg!(
                    hg_map.insert(hg_key.clone(), hg_value).is_none() || ubtype == IgnoreDuplicates,
                    "HgBlob {:?} already provided before",
                    hg_key
                );
                ensure_msg!(
                    thrift_map
                        .insert(thrift_key.clone(), thrift_value)
                        .is_none() || ubtype == IgnoreDuplicates,
                    "Thrift blob {:?} already provided before",
                    thrift_key
                );
                Ok((hg_map, thrift_map))
            },
        )
        .boxify()
}

/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::{ensure, Result};
use futures::{compat::Future01CompatExt, future::BoxFuture, FutureExt, Stream, TryStreamExt};
use futures_ext::{future::TryShared, FbTryFutureExt};

use blobrepo::BlobRepo;
use context::CoreContext;
use mercurial_revlog::manifest::ManifestContent;
use mercurial_types::{
    blobs::{UploadHgNodeHash, UploadHgTreeEntry},
    HgManifestId, HgNodeHash, HgNodeKey,
};
use mononoke_types::RepoPath;
use wirepack::TreemanifestEntry;

/// Represents data that is Mercurial-encoded and can be uploaded to the blobstore.
pub(crate) trait UploadableHgBlob {
    type Value: Send + 'static;

    fn upload(self, ctx: &CoreContext, repo: &BlobRepo) -> Result<(HgNodeKey, Self::Value)>;
}

#[derive(PartialEq, Eq)]
pub(crate) enum UploadBlobsType {
    IgnoreDuplicates,
    EnsureNoDuplicates,
}
use self::UploadBlobsType::*;

pub(crate) async fn upload_hg_blobs<'a, S, B>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    blobs: S,
    ubtype: UploadBlobsType,
) -> Result<HashMap<HgNodeKey, B::Value>>
where
    S: Stream<Item = Result<B>> + Send + 'a,
    B: UploadableHgBlob + 'a,
{
    let ignore_duplicates = ubtype == IgnoreDuplicates;
    blobs
        .try_fold(HashMap::new(), move |mut map, item| async move {
            let (key, value) = item.upload(ctx, repo)?;
            ensure!(
                map.insert(key.clone(), value).is_none() || ignore_duplicates,
                "HgBlob {:?} already provided before",
                key
            );
            Ok(map)
        })
        .await
}

impl UploadableHgBlob for TreemanifestEntry {
    // * Shared is required here because a single tree manifest can be referred to by more than
    //   one changeset, and all of those will want to refer to the corresponding future.
    type Value = (
        ManifestContent,
        Option<HgNodeHash>,
        Option<HgNodeHash>,
        TryShared<BoxFuture<'static, Result<(HgManifestId, RepoPath)>>>,
    );

    fn upload(self, ctx: &CoreContext, repo: &BlobRepo) -> Result<(HgNodeKey, Self::Value)> {
        let node_key = self.node_key;
        let manifest_content = self.manifest_content;
        let p1 = self.p1;
        let p2 = self.p2;
        // The root tree manifest is expected to have the wrong hash in hybrid mode.
        // XXX possibly remove this once hybrid mode is gone
        let upload_node_id = if node_key.path.is_root() {
            UploadHgNodeHash::Supplied(node_key.hash)
        } else {
            UploadHgNodeHash::Checked(node_key.hash)
        };
        let upload = UploadHgTreeEntry {
            upload_node_id,
            contents: self.data,
            p1: self.p1,
            p2: self.p2,
            path: node_key.path.clone(),
        };
        let (_node, value) = upload.upload(ctx.clone(), repo.get_blobstore().boxed())?;
        Ok((
            node_key,
            (
                manifest_content,
                p1,
                p2,
                value.compat().boxed().try_shared(),
            ),
        ))
    }
}

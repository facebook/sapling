/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::HashMap;

use failure_ext::{ensure, Compat};
use futures::{future::Shared, Future, Stream};
use futures_ext::{BoxFuture, FutureExt};

use blobrepo::BlobRepo;
use context::CoreContext;
use mercurial_revlog::manifest::ManifestContent;
use mercurial_types::{
    blobs::{HgBlobEntry, UploadHgNodeHash, UploadHgTreeEntry},
    HgNodeHash, HgNodeKey,
};
use mononoke_types::RepoPath;
use wirepack::TreemanifestEntry;

use crate::errors::*;

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
    repo: BlobRepo,
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
            ensure!(
                map.insert(key.clone(), value).is_none() || ubtype == IgnoreDuplicates,
                "HgBlob {:?} already provided before",
                key
            );
            Ok(map)
        })
        .boxify()
}

impl UploadableHgBlob for TreemanifestEntry {
    // * Shared is required here because a single tree manifest can be referred to by more than
    //   one changeset, and all of those will want to refer to the corresponding future.
    // * The Compat<Error> here is because the error type for Shared (a cloneable wrapper called
    //   SharedError) doesn't implement Fail, and only implements Error if the wrapped type
    //   implements Error.
    type Value = (
        ManifestContent,
        Option<HgNodeHash>,
        Option<HgNodeHash>,
        Shared<BoxFuture<(HgBlobEntry, RepoPath), Compat<Error>>>,
    );

    fn upload(self, ctx: CoreContext, repo: &BlobRepo) -> Result<(HgNodeKey, Self::Value)> {
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
        upload
            .upload(ctx, repo.get_blobstore().boxed())
            .map(move |(_node, value)| {
                (
                    node_key,
                    (
                        manifest_content,
                        p1,
                        p2,
                        value.map_err(Compat).boxify().shared(),
                    ),
                )
            })
    }
}

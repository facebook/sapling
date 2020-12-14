/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error, Result};
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::{Blobstore, Loadable};
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use derived_data::{
    impl_bonsai_derived_mapping, BlobstoreExistsMapping, BonsaiDerivable, BonsaiDerived,
    DerivedDataTypesConfig,
};
use filestore::{self, FetchKey};
use futures::{future, StreamExt, TryFutureExt, TryStreamExt};
use manifest::find_intersection_of_diffs;
use mononoke_types::{
    blame::{store_blame, Blame, BlameId, BlameRejected},
    BonsaiChangeset, ChangesetId, ContentId, FileUnodeId, MPath,
};
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;
use unodes::{find_unode_renames, RootUnodeManifestId};

pub const BLAME_FILESIZE_LIMIT: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct BlameRoot(ChangesetId);

impl From<ChangesetId> for BlameRoot {
    fn from(csid: ChangesetId) -> BlameRoot {
        BlameRoot(csid)
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct BlameDeriveOptions {
    filesize_limit: u64,
}

impl Default for BlameDeriveOptions {
    fn default() -> Self {
        BlameDeriveOptions {
            filesize_limit: BLAME_FILESIZE_LIMIT,
        }
    }
}

#[async_trait]
impl BonsaiDerivable for BlameRoot {
    const NAME: &'static str = "blame";

    type Options = BlameDeriveOptions;

    async fn derive_from_parents(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
        options: &Self::Options,
    ) -> Result<Self, Error> {
        let blame_options = *options;
        let csid = bonsai.get_changeset_id();
        let root_manifest = RootUnodeManifestId::derive(&ctx, &repo, csid)
            .map_ok(|root_id| root_id.manifest_unode_id().clone());
        let parents_manifest = bonsai
            .parents()
            .collect::<Vec<_>>() // iterator should be owned
            .into_iter()
            .map(|csid| {
                RootUnodeManifestId::derive(&ctx, &repo, csid)
                    .map_ok(|root_id| root_id.manifest_unode_id().clone())
            });

        let (root_mf, parents_mf, renames) = future::try_join3(
            root_manifest.err_into(),
            future::try_join_all(parents_manifest).err_into(),
            find_unode_renames(ctx.clone(), repo.clone(), &bonsai),
        )
        .await?;

        let renames = Arc::new(renames);
        let blobstore = repo.get_blobstore().boxed();
        find_intersection_of_diffs(ctx.clone(), blobstore, root_mf, parents_mf)
            .map_ok(|(path, entry)| Some((path?, entry.into_leaf()?)))
            .try_filter_map(future::ok)
            .map(move |v| {
                cloned!(ctx, repo, renames);
                async move {
                    let (path, file) = v?;
                    Result::<_>::Ok(
                        tokio::spawn(async move {
                            create_blame(&ctx, &repo, renames, csid, path, file, blame_options)
                                .await
                        })
                        .await??,
                    )
                }
            })
            .buffered(256)
            .try_for_each(|_| future::ok(()))
            .await?;

        Ok(BlameRoot(csid))
    }
}

#[derive(Clone)]
pub struct BlameRootMapping {
    blobstore: Arc<dyn Blobstore>,
    options: BlameDeriveOptions,
}

#[async_trait]
impl BlobstoreExistsMapping for BlameRootMapping {
    type Value = BlameRoot;

    fn new(repo: &BlobRepo, config: &DerivedDataTypesConfig) -> Result<Self> {
        let filesize_limit = config.blame_filesize_limit.unwrap_or(BLAME_FILESIZE_LIMIT);
        let options = BlameDeriveOptions { filesize_limit };
        Ok(Self {
            blobstore: repo.get_blobstore().boxed(),
            options,
        })
    }

    fn blobstore(&self) -> &dyn Blobstore {
        &self.blobstore
    }

    fn prefix(&self) -> &'static str {
        "derived_rootblame.v1."
    }

    fn options(&self) -> BlameDeriveOptions {
        self.options
    }
}

impl_bonsai_derived_mapping!(BlameRootMapping, BlobstoreExistsMapping, BlameRoot);

async fn create_blame(
    ctx: &CoreContext,
    repo: &BlobRepo,
    renames: Arc<HashMap<MPath, FileUnodeId>>,
    csid: ChangesetId,
    path: MPath,
    file_unode_id: FileUnodeId,
    options: BlameDeriveOptions,
) -> Result<BlameId, Error> {
    let blobstore = repo.blobstore();

    let file_unode = file_unode_id.load(ctx, blobstore).await?;

    let parents_content_and_blame: Vec<_> = file_unode
        .parents()
        .iter()
        .cloned()
        .chain(renames.get(&path).cloned())
        .map(|file_unode_id| {
            future::try_join(
                fetch_file_full_content(ctx, repo, file_unode_id, options),
                async move { BlameId::from(file_unode_id).load(ctx, blobstore).await }.err_into(),
            )
        })
        .collect();

    let (content, parents_content) = future::try_join(
        fetch_file_full_content(ctx, repo, file_unode_id, options),
        future::try_join_all(parents_content_and_blame),
    )
    .await?;

    let blame_maybe_rejected = match content {
        Err(rejected) => rejected.into(),
        Ok(content) => {
            let parents_content = parents_content
                .into_iter()
                .filter_map(|(content, blame_maybe_rejected)| {
                    Some((content.ok()?, blame_maybe_rejected.into_blame().ok()?))
                })
                .collect();
            Blame::from_parents(csid, content, path, parents_content)?.into()
        }
    };

    store_blame(ctx, &blobstore, file_unode_id, blame_maybe_rejected).await
}

pub async fn fetch_file_full_content(
    ctx: &CoreContext,
    repo: &BlobRepo,
    file_unode_id: FileUnodeId,
    options: BlameDeriveOptions,
) -> Result<Result<Bytes, BlameRejected>, Error> {
    let blobstore = repo.blobstore();
    let file_unode = file_unode_id
        .load(ctx, blobstore)
        .map_err(|error| FetchError::Error(error.into()))
        .await?;

    let content_id = *file_unode.content_id();
    let result = fetch_from_filestore(ctx, repo, content_id, options).await;

    match result {
        Err(FetchError::Error(error)) => Err(error),
        Err(FetchError::Rejected(rejected)) => Ok(Err(rejected)),
        Ok(content) => Ok(Ok(content)),
    }
}

#[derive(Error, Debug)]
enum FetchError {
    #[error("FetchError::Rejected")]
    Rejected(#[source] BlameRejected),
    #[error("FetchError::Error")]
    Error(#[source] Error),
}

fn check_binary(content: &[u8]) -> Result<&[u8], FetchError> {
    if content.contains(&0u8) {
        Err(FetchError::Rejected(BlameRejected::Binary))
    } else {
        Ok(content)
    }
}

async fn fetch_from_filestore(
    ctx: &CoreContext,
    repo: &BlobRepo,
    content_id: ContentId,
    options: BlameDeriveOptions,
) -> Result<Bytes, FetchError> {
    let result = filestore::fetch_with_size(
        repo.get_blobstore(),
        ctx.clone(),
        &FetchKey::Canonical(content_id),
    )
    .map_err(FetchError::Error)
    .await?;

    match result {
        None => {
            let error = FetchError::Error(format_err!("Missing content: {}", content_id));
            Err(error)
        }
        Some((stream, size)) => {
            if size > options.filesize_limit {
                return Err(FetchError::Rejected(BlameRejected::TooBig));
            }
            let v = Vec::with_capacity(size as usize);
            let bytes = stream
                .map_err(FetchError::Error)
                .try_fold(v, |mut acc, bytes| async move {
                    acc.extend(check_binary(bytes.as_ref())?);
                    Ok(acc)
                })
                .map_ok(Bytes::from)
                .await?;
            Ok(bytes)
        }
    }
}

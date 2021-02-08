/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Context as _, Error};
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use bytes::Bytes;
use context::CoreContext;
use futures::{future, stream::TryStreamExt};
use futures_util::future::TryFutureExt;
use manifest::{Entry, ManifestOps};
use mercurial_types::{FileType, HgFileNodeId, HgManifestId};
use mononoke_types::{ContentId, MPath};
use std::collections::HashMap;

use crate::{ErrorKind, FileContentFetcher, PathContent};

pub struct BlobRepoFileContentFetcher {
    pub repo: BlobRepo,
}

#[async_trait]
impl FileContentFetcher for BlobRepoFileContentFetcher {
    async fn get_file_size<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<u64, ErrorKind> {
        let store = self.repo.blobstore();
        Ok(filestore::get_metadata(store, ctx, &id.into())
            .await?
            .ok_or(ErrorKind::ContentIdNotFound(id))?
            .total_size)
    }

    async fn get_file_text<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>, ErrorKind> {
        let store = self.repo.blobstore();
        filestore::fetch_concat_opt(store, ctx, &id.into())
            .await?
            .ok_or(ErrorKind::ContentIdNotFound(id))
            .map(Option::Some)
    }

    async fn find_content<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: BookmarkName,
        paths: Vec<MPath>,
    ) -> Result<HashMap<MPath, PathContent>, ErrorKind> {
        let master_mf = derive_manifest_for_bookmark(ctx, &self.repo, &bookmark).await?;
        master_mf
            .find_entries(ctx.clone(), self.repo.get_blobstore(), paths)
            .map_ok(|(mb_path, entry)| async move {
                if let Some(path) = mb_path {
                    let content = resolve_content_id(ctx, &self.repo, entry).await?;
                    Ok(Some((path, content)))
                } else {
                    Ok(None)
                }
            })
            .try_buffer_unordered(100)
            .try_filter_map(future::ok)
            .try_collect::<HashMap<_, _>>()
            .map_err(ErrorKind::from)
            .await
    }
}

impl BlobRepoFileContentFetcher {
    pub fn new(repo: BlobRepo) -> BlobRepoFileContentFetcher {
        BlobRepoFileContentFetcher { repo }
    }
}

async fn derive_manifest_for_bookmark(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmark: &BookmarkName,
) -> Result<HgManifestId, ErrorKind> {
    let changeset_id = repo
        .get_bonsai_bookmark(ctx.clone(), &bookmark)
        .await
        .with_context(|| format!("Error fetching bookmark: {}", bookmark))?
        .ok_or_else(|| format_err!("Bookmark {} does not exist", bookmark))?;

    let hg_changeset_id = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), changeset_id)
        .await
        .with_context(|| format!("Error deriving hg changeset for bonsai: {}", changeset_id))?;
    let hg_mf_id = hg_changeset_id
        .load(&ctx, &repo.get_blobstore())
        .map_ok(|hg_changeset| hg_changeset.manifestid())
        .await
        .with_context(|| format!("Error loading hg changeset: {}", hg_changeset_id))?;

    Ok(hg_mf_id)
}

async fn resolve_content_id(
    ctx: &CoreContext,
    repo: &BlobRepo,
    entry: Entry<HgManifestId, (FileType, HgFileNodeId)>,
) -> Result<PathContent, Error> {
    match entry {
        Entry::Tree(_tree) => {
            // there is no content for trees
            Ok(PathContent::Directory)
        }
        Entry::Leaf((_type, file_node_id)) => file_node_id
            .load(ctx, &repo.get_blobstore())
            .map_ok(|file_env| PathContent::File(file_env.content_id()))
            .await
            .with_context(|| format!("Error loading filenode: {}", file_node_id)),
    }
}

/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bytes::Bytes;
use context::CoreContext;
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future,
    stream::TryStreamExt,
};
use manifest::{Diff, Entry, ManifestOps};
use mercurial_types::{blobs::HgBlobChangeset, FileBytes, HgChangesetId, HgFileNodeId, MPath};
use mononoke_types::ContentId;
use mononoke_types::FileType;

use crate::{ChangedFileType, ChangesetStore, ErrorKind, FileContentFetcher, FileContentStore};

// TODO this can cache file content locally to prevent unnecessary lookup of changeset,
// manifest and walk of manifest each time
// It's likely that multiple hooks will want to see the same content for the same changeset
pub struct BlobRepoFileContentStore {
    pub repo: BlobRepo,
}

pub struct BlobRepoChangesetStore {
    pub repo: BlobRepo,
}

#[async_trait]
impl FileContentStore for BlobRepoFileContentStore {
    async fn resolve_path<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        changeset_id: HgChangesetId,
        path: MPath,
    ) -> Result<Option<HgFileNodeId>, Error> {
        let cs = changeset_id
            .load(ctx.clone(), self.repo.blobstore())
            .compat()
            .await?;
        let entry = cs
            .manifestid()
            .find_entry(ctx.clone(), self.repo.get_blobstore(), Some(path))
            .compat()
            .await?;
        Ok(entry.and_then(|entry| entry.into_leaf()).map(|leaf| leaf.1))
    }

    async fn get_file_text<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        id: HgFileNodeId,
    ) -> Result<Option<FileBytes>, Error> {
        let store = self.repo.get_blobstore();
        let envelope = id.load(ctx.clone(), &store).compat().await?;
        let content = filestore::fetch_concat(&store, ctx.clone(), envelope.content_id())
            .compat()
            .await?;
        Ok(Some(FileBytes(content)))
    }

    async fn get_file_size<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        id: HgFileNodeId,
    ) -> Result<u64, Error> {
        let envelope = id.load(ctx.clone(), self.repo.blobstore()).compat().await?;
        Ok(envelope.content_size())
    }
}

impl BlobRepoFileContentStore {
    pub fn new(repo: BlobRepo) -> BlobRepoFileContentStore {
        BlobRepoFileContentStore { repo }
    }
}

#[async_trait]
impl ChangesetStore for BlobRepoChangesetStore {
    async fn get_changeset_by_changesetid<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        changesetid: HgChangesetId,
    ) -> Result<HgBlobChangeset, Error> {
        changesetid
            .load(ctx.clone(), self.repo.blobstore())
            .compat()
            .await
            .map_err(|e| e.into())
    }

    async fn get_changed_files<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        changesetid: HgChangesetId,
    ) -> Result<Vec<(String, ChangedFileType, Option<(HgFileNodeId, FileType)>)>, Error> {
        let cs = changesetid
            .load(ctx.clone(), self.repo.blobstore())
            .compat()
            .await?;
        let mf_id = cs.manifestid();
        let parents = cs.parents();
        let (maybe_p1, _) = parents.get_nodes();
        match maybe_p1 {
            Some(p1) => {
                let p1 = HgChangesetId::new(p1)
                    .load(ctx.clone(), self.repo.blobstore())
                    .compat()
                    .await?;
                let p_mf_id = p1.manifestid();
                p_mf_id
                    .diff(ctx.clone(), self.repo.get_blobstore(), mf_id)
                    .compat()
                    .try_filter_map(|diff| {
                        let (path, change_type, entry) = match diff {
                            Diff::Added(path, entry) => (path, ChangedFileType::Added, entry),
                            Diff::Removed(path, entry) => (path, ChangedFileType::Deleted, entry),
                            Diff::Changed(path, .., entry) => {
                                (path, ChangedFileType::Modified, entry)
                            }
                        };

                        match (change_type, entry) {
                            (ChangedFileType::Deleted, Entry::Leaf(_)) => {
                                future::ok(Some((path, ChangedFileType::Deleted, None)))
                            }
                            (change_type, Entry::Leaf((ty, hash))) => {
                                future::ok(Some((path, change_type, Some((hash, ty)))))
                            }
                            (_, Entry::Tree(_)) => future::ok(None),
                        }
                    })
                    .try_filter_map(|(maybe_path, ty, hash_and_type)| {
                        future::ok(maybe_path.map(|path| {
                            (
                                String::from_utf8_lossy(&path.to_vec()).into_owned(),
                                ty,
                                hash_and_type,
                            )
                        }))
                    })
                    .try_collect()
                    .await
            }
            None => {
                mf_id
                    .list_leaf_entries(ctx.clone(), self.repo.get_blobstore())
                    .compat()
                    .map_ok(|(path, (ty, filenode))| {
                        (
                            String::from_utf8_lossy(&path.to_vec()).into_owned(),
                            ChangedFileType::Added,
                            Some((filenode, ty)),
                        )
                    })
                    .try_collect()
                    .await
            }
        }
    }
}

impl BlobRepoChangesetStore {
    pub fn new(repo: BlobRepo) -> BlobRepoChangesetStore {
        BlobRepoChangesetStore { repo }
    }
}

pub struct BlobRepoFileContentFetcher {
    pub repo: BlobRepo,
}

#[async_trait]
impl FileContentFetcher for BlobRepoFileContentFetcher {
    async fn get_file_size<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        id: ContentId,
    ) -> Result<u64, ErrorKind> {
        let store = self.repo.get_blobstore();
        Ok(filestore::get_metadata(&store, ctx.clone(), &id.into())
            .compat()
            .await?
            .ok_or(ErrorKind::ContentIdNotFound(id))?
            .total_size)
    }

    async fn get_file_text<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>, ErrorKind> {
        let store = self.repo.get_blobstore();
        filestore::fetch_concat_opt(&store, ctx.clone(), &id.into())
            .compat()
            .await?
            .ok_or(ErrorKind::ContentIdNotFound(id))
            .map(Option::Some)
    }
}

impl BlobRepoFileContentFetcher {
    pub fn new(repo: BlobRepo) -> BlobRepoFileContentFetcher {
        BlobRepoFileContentFetcher { repo }
    }
}

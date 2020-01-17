/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use futures::{Future, Stream};
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use manifest::{Diff, Entry, ManifestOps};
use mercurial_types::{blobs::HgBlobChangeset, FileBytes, HgChangesetId, HgFileNodeId, MPath};
use mononoke_types::FileType;

use crate::{ChangedFileType, ChangesetStore, FileContentStore};

// TODO this can cache file content locally to prevent unnecessary lookup of changeset,
// manifest and walk of manifest each time
// It's likely that multiple hooks will want to see the same content for the same changeset
pub struct BlobRepoFileContentStore {
    pub repo: BlobRepo,
}

pub struct BlobRepoChangesetStore {
    pub repo: BlobRepo,
}

impl FileContentStore for BlobRepoFileContentStore {
    fn resolve_path(
        &self,
        ctx: CoreContext,
        changeset_id: HgChangesetId,
        path: MPath,
    ) -> BoxFuture<Option<HgFileNodeId>, Error> {
        self.repo
            .get_changeset_by_changesetid(ctx.clone(), changeset_id)
            .and_then({
                cloned!(self.repo, ctx);
                move |changeset| {
                    repo.find_files_in_manifest(ctx, changeset.manifestid(), vec![path.clone()])
                        .map(move |fs| fs.get(&path).copied())
                }
            })
            .boxify()
    }

    fn get_file_text(
        &self,
        ctx: CoreContext,
        id: HgFileNodeId,
    ) -> BoxFuture<Option<FileBytes>, Error> {
        self.repo
            .get_file_content(ctx, id)
            .concat2()
            .map(Some)
            .boxify()
    }

    fn get_file_size(&self, ctx: CoreContext, id: HgFileNodeId) -> BoxFuture<u64, Error> {
        self.repo.get_file_size(ctx, id)
    }
}

impl BlobRepoFileContentStore {
    pub fn new(repo: BlobRepo) -> BlobRepoFileContentStore {
        BlobRepoFileContentStore { repo }
    }
}

impl ChangesetStore for BlobRepoChangesetStore {
    fn get_changeset_by_changesetid(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> BoxFuture<HgBlobChangeset, Error> {
        self.repo.get_changeset_by_changesetid(ctx, changesetid)
    }

    fn get_changed_files(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> BoxFuture<Vec<(String, ChangedFileType, Option<(HgFileNodeId, FileType)>)>, Error> {
        cloned!(self.repo);
        self.repo
            .get_changeset_by_changesetid(ctx.clone(), changesetid)
            .map({
                cloned!(ctx);
                move |cs| {
                    let mf_id = cs.manifestid();
                    let parents = cs.parents();
                    let (maybe_p1, _) = parents.get_nodes();
                    match maybe_p1 {
                        Some(p1) => repo
                            .get_changeset_by_changesetid(ctx.clone(), HgChangesetId::new(p1))
                            .map(|p1| p1.manifestid())
                            .map({
                                cloned!(repo);
                                move |p_mf_id| {
                                    p_mf_id.diff(ctx.clone(), repo.get_blobstore(), mf_id)
                                }
                            })
                            .flatten_stream()
                            .filter_map(|diff| {
                                let (path, entry) = match diff.clone() {
                                    Diff::Added(path, entry) => (path, entry),
                                    Diff::Removed(path, entry) => (path, entry),
                                    Diff::Changed(path, .., entry) => (path, entry),
                                };

                                let hash_and_type = match entry {
                                    Entry::Leaf((ty, hash)) => (hash, ty),
                                    Entry::Tree(_) => {
                                        return None;
                                    }
                                };

                                match diff {
                                    Diff::Added(..) => {
                                        Some((path, ChangedFileType::Added, Some(hash_and_type)))
                                    }
                                    Diff::Changed(..) => {
                                        Some((path, ChangedFileType::Modified, Some(hash_and_type)))
                                    }
                                    Diff::Removed(..) => {
                                        Some((path, ChangedFileType::Deleted, None))
                                    }
                                }
                            })
                            .filter_map(|(maybe_path, ty, hash_and_type)| {
                                maybe_path.map(|path| (path, ty, hash_and_type))
                            })
                            .boxify(),
                        None => mf_id
                            .list_leaf_entries(ctx.clone(), repo.get_blobstore())
                            .map(|(path, (ty, filenode))| {
                                (path, ChangedFileType::Added, Some((filenode, ty)))
                            })
                            .boxify(),
                    }
                }
            })
            .flatten_stream()
            .map(|(path, ty, hash_and_type)| {
                (
                    String::from_utf8_lossy(&path.to_vec()).into_owned(),
                    ty,
                    hash_and_type,
                )
            })
            .collect()
            .boxify()
    }
}

impl BlobRepoChangesetStore {
    pub fn new(repo: BlobRepo) -> BlobRepoChangesetStore {
        BlobRepoChangesetStore { repo }
    }
}

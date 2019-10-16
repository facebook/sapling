/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! This crate contains the traits for interactive with Hook manager

#![deny(warnings)]

use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use failure_ext::Error;
use futures::{future, Future, Stream};
use futures_ext::{BoxFuture, BoxStream, FutureExt};
use hooks::{ChangedFileType, ChangesetStore, FileContentStore};
use mercurial_types::manifest_utils::{self, EntryStatus};
use mercurial_types::{
    blobs::HgBlobChangeset, manifest::get_empty_manifest, Changeset, FileBytes, HgChangesetId,
    HgFileNodeId, MPath, Type,
};
use mononoke_types::FileType;

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

    fn stream_file_contents(
        &self,
        ctx: CoreContext,
        id: HgFileNodeId,
    ) -> BoxStream<FileBytes, Error> {
        self.repo.get_file_content(ctx, id)
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
            .and_then({
                cloned!(ctx);
                move |cs| {
                    let mf_id = cs.manifestid();
                    let mf = repo.get_manifest_by_nodeid(ctx.clone(), mf_id);
                    let parents = cs.parents();
                    let (maybe_p1, _) = parents.get_nodes();
                    // TODO(stash): generate changed file stream correctly for merges
                    let p_mf = match maybe_p1 {
                        Some(p1) => repo
                            .get_changeset_by_changesetid(ctx.clone(), HgChangesetId::new(p1))
                            .and_then({
                                cloned!(repo);
                                move |p1| repo.get_manifest_by_nodeid(ctx, p1.manifestid())
                            })
                            .left_future(),
                        None => future::ok(get_empty_manifest()).right_future(),
                    };
                    (mf, p_mf)
                }
            })
            .and_then(move |(mf, p_mf)| {
                manifest_utils::changed_file_stream(ctx, &mf, &p_mf, None)
                    .map(|changed_entry| {
                        let path = changed_entry
                            .get_full_path()
                            .expect("File should have a path");
                        let entry = match &changed_entry.status {
                            EntryStatus::Added(entry) => Some(entry),
                            EntryStatus::Deleted(_entry) => None,
                            EntryStatus::Modified { to_entry, .. } => Some(to_entry),
                        };

                        let hash_and_type = entry.map(|entry| {
                            let file_type = match entry.get_type() {
                                Type::File(file_type) => file_type,
                                Type::Tree => {
                                    panic!("unexpected tree returned");
                                }
                            };

                            let filenode = HgFileNodeId::new(entry.get_hash().into_nodehash());
                            (filenode, file_type)
                        });

                        let change_ty = ChangedFileType::from(changed_entry.status);
                        (
                            String::from_utf8_lossy(&path.to_vec()).into_owned(),
                            change_ty,
                            hash_and_type,
                        )
                    })
                    .collect()
            })
            .boxify()
    }
}

impl BlobRepoChangesetStore {
    pub fn new(repo: BlobRepo) -> BlobRepoChangesetStore {
        BlobRepoChangesetStore { repo }
    }
}

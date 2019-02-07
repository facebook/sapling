// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This crate contains the traits for interactive with Hook manager

#![deny(warnings)]

use blobrepo::{BlobRepo, HgBlobChangeset};
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use failure_ext::Error;
use futures::{finished, Future, Stream};
use futures_ext::{BoxFuture, FutureExt};
use hooks::{ChangedFileType, ChangesetStore, FileContentStore};
use mercurial_types::manifest_utils;
use mercurial_types::{manifest::get_empty_manifest, Changeset, HgChangesetId, MPath};
use mononoke_types::{FileContents, FileType};

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
    fn get_file_content_for_changeset(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
        path: MPath,
    ) -> BoxFuture<Option<(FileType, Bytes)>, Error> {
        let repo = self.repo.clone();
        let repo2 = repo.clone();
        repo.get_changeset_by_changesetid(ctx.clone(), changesetid)
            .and_then({
                cloned!(ctx);
                move |changeset| {
                    repo.find_file_in_manifest(ctx, &path, changeset.manifestid().clone())
                }
            })
            .and_then(move |opt| match opt {
                Some((file_type, hash)) => repo2
                    .get_file_content(ctx, hash)
                    .map(move |content| Some((file_type, content)))
                    .boxify(),
                None => finished(None).boxify(),
            })
            .and_then(|opt| match opt {
                Some((file_type, content)) => {
                    let FileContents::Bytes(bytes) = content;
                    Ok(Some((file_type, bytes)))
                }
                None => Ok(None),
            })
            .boxify()
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
    ) -> BoxFuture<Vec<(String, ChangedFileType)>, Error> {
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
                        Some(p1) => {
                            repo.get_changeset_by_changesetid(ctx.clone(), HgChangesetId::new(p1))
                                .and_then({
                                    cloned!(repo);
                                    move |p1| repo.get_manifest_by_nodeid(ctx, p1.manifestid())
                                })
                                .left_future()
                        }
                        None => finished(get_empty_manifest()).right_future(),
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
                        let ty = ChangedFileType::from(changed_entry.status);
                        (String::from_utf8_lossy(&path.to_vec()).into_owned(), ty)
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

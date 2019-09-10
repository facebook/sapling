// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::BTreeMap;

use cloned::cloned;
use failure_ext::prelude::*;
use futures::future::{join_all, Future};
use futures::{IntoFuture, Stream};
use futures_ext::{try_boxfuture, FutureExt};

use blobstore::Blobstore;
use context::CoreContext;
use mercurial_types::{
    blobs::{HgBlobChangeset, HgBlobEnvelope},
    Changeset, HgEntry, HgFileEnvelope, HgFileNodeId, HgManifestId, MPath, RepoPath,
};
use mononoke_types::{
    BlobstoreValue, BonsaiChangeset, BonsaiChangesetMut, ChangesetId, FileChange, MononokeId,
};
use repo_blobstore::RepoBlobstore;

use crate::errors::*;
use crate::BlobRepo;

/// Creates bonsai changeset from already created HgBlobChangeset.
pub fn create_bonsai_changeset_object(
    ctx: CoreContext,
    cs: HgBlobChangeset,
    parent_manifests: Vec<HgManifestId>,
    bonsai_parents: Vec<ChangesetId>,
    repo: BlobRepo,
) -> impl Future<Item = BonsaiChangeset, Error = Error> {
    let file_changes = find_file_changes(
        ctx,
        cs.clone(),
        parent_manifests,
        repo.clone(),
        bonsai_parents.clone(),
    );

    file_changes.and_then({
        let cs = cs.clone();
        let parents = bonsai_parents.clone();
        move |file_changes| {
            let mut extra = BTreeMap::new();
            for (key, value) in cs.extra() {
                // Hg changesets can have non-utf8 extras, but we don't allow them in Bonsai
                // In that case convert them lossy.
                let key = String::from_utf8(key.clone())?.to_string();
                extra.insert(key, value.clone());
            }

            let author = String::from_utf8(cs.user().to_vec())
                .with_context(|_| format!("While converting author name {:?}", cs.user()))?;
            let message = String::from_utf8(cs.comments().to_vec())
                .with_context(|_| format!("While converting commit message {:?}", cs.comments()))?;
            BonsaiChangesetMut {
                parents,
                author,
                author_date: *cs.time(),
                committer: None,
                committer_date: None,
                message,
                extra,
                file_changes,
            }
            .freeze()
        }
    })
}

pub fn save_bonsai_changeset_object(
    ctx: CoreContext,
    blobstore: RepoBlobstore,
    bonsai_cs: BonsaiChangeset,
) -> impl Future<Item = (), Error = Error> {
    let bonsai_blob = bonsai_cs.into_blob();
    let bcs_id = bonsai_blob.id().clone();
    let blobstore_key = bcs_id.blobstore_key();

    blobstore
        .put(ctx, blobstore_key, bonsai_blob.into())
        .map(|_| ())
}

// Finds files that were changed in the commit and returns it in the format suitable for BonsaiChangeset
fn find_file_changes(
    ctx: CoreContext,
    cs: HgBlobChangeset,
    parent_manifests: Vec<HgManifestId>,
    repo: BlobRepo,
    bonsai_parents: Vec<ChangesetId>,
) -> impl Future<Item = BTreeMap<MPath, Option<FileChange>>, Error = Error> {
    let root_entry = Box::new(repo.get_root_entry(cs.manifestid()));

    let p1_root_entry = parent_manifests
        .get(0)
        .map(|root_mf| -> Box<dyn HgEntry + Sync> { Box::new(repo.get_root_entry(*root_mf)) });
    let p2_root_entry: Option<Box<dyn HgEntry + Sync>> = parent_manifests.get(1).map(|root_mf| {
        let entry: Box<dyn HgEntry + Sync> = Box::new(repo.get_root_entry(*root_mf));
        entry
    });

    bonsai_utils::bonsai_diff(ctx.clone(), root_entry, p1_root_entry, p2_root_entry)
        .map(move |changed_file| match changed_file {
            bonsai_utils::BonsaiDiffResult::Changed(path, ty, entry_id) => {
                let file_node_id = HgFileNodeId::new(entry_id.into_nodehash());
                cloned!(ctx, bonsai_parents, repo, parent_manifests);
                repo.get_file_envelope(ctx.clone(), file_node_id)
                    .and_then(move |envelope| {
                        let size = envelope.content_size();
                        let content_id = envelope.content_id();

                        get_copy_info(
                            ctx,
                            repo,
                            bonsai_parents,
                            path.clone(),
                            envelope,
                            parent_manifests,
                        ).context("While fetching copy information")
                            .from_err()
                            .map(move |copyinfo| {
                                (
                                    path,
                                    Some(FileChange::new(content_id, ty, size as u64, copyinfo)),
                                )
                            })
                    })
                    .boxify()
            }
            bonsai_utils::BonsaiDiffResult::ChangedReusedId(path, ty, entry_id) => {
                let file_node_id = HgFileNodeId::new(entry_id.into_nodehash());
                cloned!(ctx, repo);
                repo.get_file_envelope(ctx, file_node_id).and_then(move |envelope| {
                    let size = envelope.content_size();
                    let content_id = envelope.content_id();

                    // Reused ID means copy info is *not* stored.
                    Ok((path, Some(FileChange::new(content_id, ty, size as u64, None))))
                }).boxify()
            }
            bonsai_utils::BonsaiDiffResult::Deleted(path) => {
                Ok((path, None)).into_future().boxify()
            }
        })
        .buffer_unordered(100) // TODO(stash): magic number?
        .collect()
        .map(|paths| {
            let paths: BTreeMap<_, _> = paths.into_iter().collect();
            paths
        })
        .context("While fetching bonsai file changes")
        .from_err()
}

// Returns copy information for a given path and node if this file was copied.
// This function is quite complicated because hg and bonsai store copy information differently.
// In hg copy information is (path, filenode), in bonsai it's (path, parent cs id). That means that
// we need to find a parent from which this filenode was copied.
fn get_copy_info(
    ctx: CoreContext,
    repo: BlobRepo,
    bonsai_parents: Vec<ChangesetId>,
    copy_from_path: MPath,
    envelope: HgFileEnvelope,
    parent_manifests: Vec<HgManifestId>,
) -> impl Future<Item = Option<(MPath, ChangesetId)>, Error = Error> {
    let node_id = envelope.node_id();

    let maybecopy = try_boxfuture!(envelope.get_copy_info())
        .map(|(path, hash)| (RepoPath::FilePath(path), hash));

    match maybecopy {
        Some((repopath, copyfromnode)) => {
            let repopath: Result<MPath> = repopath
                .mpath()
                .cloned()
                .ok_or(ErrorKind::UnexpectedRootPath.into());

            let parents_bonsai_and_mfs =
                bonsai_parents.into_iter().zip(parent_manifests.into_iter());

            repopath
                .into_future()
                .and_then(move |repopath| {
                    join_all(parents_bonsai_and_mfs.map({
                        cloned!(ctx, repopath);
                        move |(bonsai_parent, parent_mf)| {
                            repo.find_files_in_manifest(
                                ctx.clone(),
                                parent_mf,
                                vec![repopath.clone()],
                            )
                            .map({
                                cloned!(repopath);
                                move |res| {
                                    if res.get(&repopath).copied() == Some(copyfromnode) {
                                        Some(bonsai_parent)
                                    } else {
                                        None
                                    }
                                }
                            })
                        }
                    }))
                    .map(move |res| (res, repopath))
                })
                .and_then(move |(copied_from_bonsai_commits, repopath)| {
                    let copied_from: Vec<_> = copied_from_bonsai_commits
                        .into_iter()
                        .filter_map(|x| x)
                        .collect();
                    match copied_from.get(0) {
                        Some(bonsai_cs_copied_from) => {
                            Ok(Some((repopath, bonsai_cs_copied_from.clone())))
                        }
                        None => Err(ErrorKind::IncorrectCopyInfo {
                            from_path: copy_from_path,
                            from_node: node_id,
                            to_path: repopath.clone(),
                            to_node: copyfromnode,
                        }
                        .into()),
                    }
                })
                .boxify()
        }
        None => Ok(None).into_future().boxify(),
    }
}

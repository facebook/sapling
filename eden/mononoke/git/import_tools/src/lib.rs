/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod git_pool;
mod gitimport_objects;

pub use crate::git_pool::GitPool;
pub use crate::gitimport_objects::{
    oid_to_sha1, CommitMetadata, ExtractedCommit, FullRepoImport, GitLeaf, GitManifest,
    GitRangeImport, GitTree, GitimportPreferences, GitimportTarget, ImportMissingForCommit,
};
use anyhow::{format_err, Context, Error};
use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobrepo_hg::BlobRepoHg;
use blobstore::Blobstore;
use bonsai_git_mapping::BonsaiGitMappingEntry;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use derived_data::BonsaiDerived;
use filestore::{self, Alias, FetchKey, FilestoreConfig, StoreRequest};
use futures::{future, stream, Stream, StreamExt, TryStreamExt};
use git2::{Oid, Repository, Sort};
use git_types::TreeHandle;
use linked_hash_map::LinkedHashMap;
use manifest::{bonsai_diff, BonsaiDiffFileChange, StoreLoadable};
use mercurial_derived_data::get_manifest_from_bonsai;
use mercurial_types::HgManifestId;
use mononoke_types::{
    hash, BonsaiChangeset, BonsaiChangesetMut, ChangesetId, ContentMetadata, FileChange, MPath,
};
use slog::{debug, info};
use std::collections::{BTreeMap, HashMap};
use std::convert::TryInto;
use std::path::Path;
use tokio::task;

const HGGIT_COMMIT_ID_EXTRA: &str = "convert_revision";

async fn do_upload<B: Blobstore + Clone + 'static>(
    ctx: &CoreContext,
    blobstore: &B,
    pool: GitPool,
    oid: Oid,
) -> Result<ContentMetadata, Error> {
    // First lets see if we already have the blob in Mononoke.
    let sha1 = oid_to_sha1(&oid)?;
    if let Some(meta) =
        filestore::get_metadata(blobstore, ctx, &FetchKey::from(Alias::GitSha1(sha1))).await?
    {
        debug!(
            ctx.logger(),
            "Found git-blob:{} size:{} in blostore.",
            sha1.to_brief(),
            meta.total_size,
        );
        return Ok(meta);
    }

    // Blob not already in Mononoke, lets upload it.
    let (id, bytes) = pool
        .with(move |repo| {
            let blob = repo.find_blob(oid)?;
            let bytes = Bytes::copy_from_slice(blob.content());
            let id = blob.id();
            Result::<_, Error>::Ok((id, bytes))
        })
        .await?;

    let size = bytes.len().try_into()?;
    let git_sha1 =
        hash::RichGitSha1::from_bytes(Bytes::copy_from_slice(id.as_bytes()), "blob", size)?;
    let req = StoreRequest::with_git_sha1(size, git_sha1);
    debug!(
        ctx.logger(),
        "Uploading git-blob:{} size:{}",
        sha1.to_brief(),
        size
    );
    let meta = filestore::store(
        blobstore,
        FilestoreConfig::default(),
        ctx,
        &req,
        stream::once(async move { Ok(bytes) }),
    )
    .await?;

    Ok(meta)
}

// TODO: Try to produce copy-info?
// TODO: Translate LFS pointers?
async fn find_file_changes<S, B: Blobstore + Clone + 'static>(
    ctx: &CoreContext,
    blobstore: &B,
    pool: GitPool,
    changes: S,
) -> Result<BTreeMap<MPath, Option<FileChange>>, Error>
where
    S: Stream<Item = Result<BonsaiDiffFileChange<GitLeaf>, Error>>,
{
    changes
        .map_ok(move |change| {
            cloned!(pool);
            async move {
                match change {
                    BonsaiDiffFileChange::Changed(path, ty, GitLeaf(oid))
                    | BonsaiDiffFileChange::ChangedReusedId(path, ty, GitLeaf(oid)) => {
                        let meta = do_upload(ctx, blobstore, pool, oid).await?;
                        Ok((
                            path,
                            Some(FileChange::new(meta.content_id, ty, meta.total_size, None)),
                        ))
                    }
                    BonsaiDiffFileChange::Deleted(path) => Ok((path, None)),
                }
            }
        })
        .try_buffer_unordered(100)
        .try_collect()
        .await
}

pub async fn gitimport(
    ctx: &CoreContext,
    repo: &BlobRepo,
    path: &Path,
    target: &dyn GitimportTarget,
    prefs: GitimportPreferences,
) -> Result<LinkedHashMap<Oid, (ChangesetId, BonsaiChangeset)>, Error> {
    let repo_name = if let Some(name) = &prefs.gitrepo_name {
        String::from(name)
    } else {
        String::from(path.file_name().unwrap_or_default().to_string_lossy())
    };
    let repo_name_ref = &repo_name;

    let walk_repo = Repository::open(&path)?;
    let pool = &GitPool::new(path.to_path_buf())?;

    let mut walk = walk_repo.revwalk()?;
    walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)?;
    target.populate_walk(&walk_repo, &mut walk)?;

    // TODO: Don't import everything in one go. Instead, hide things we already imported from the
    // traversal.

    let roots = &target.get_roots()?;
    let nb_commits_to_import = target.get_nb_commits(&walk_repo)?;
    if 0 == nb_commits_to_import {
        info!(ctx.logger(), "Nothing to import for repo {}.", repo_name);
        return Ok(LinkedHashMap::new());
    }

    // Kick off a stream that consumes the walk and prepared commits. Then, produce the Bonsais.

    // TODO: Make concurrency configurable below.

    let prefs = &prefs;
    let import_map: LinkedHashMap<Oid, (ChangesetId, BonsaiChangeset)> = stream::iter(walk)
        .map(|oid| async move {
            let oid = oid.with_context(|| "While walking commits")?;

            let ExtractedCommit {
                metadata,
                tree,
                parent_trees,
            } = ExtractedCommit::new(oid, pool)
                .await
                .with_context(|| format!("While extracting {}", oid))?;

            let file_changes = task::spawn({
                cloned!(ctx, repo, pool);
                async move {
                    find_file_changes(
                        &ctx,
                        repo.blobstore(),
                        pool.clone(),
                        bonsai_diff(ctx.clone(), pool, tree, parent_trees),
                    )
                    .await
                }
            })
            .await??;

            Ok((metadata, file_changes))
        })
        .buffered(20)
        .try_fold(
            LinkedHashMap::<Oid, (ChangesetId, BonsaiChangeset)>::new(),
            {
                move |mut import_map, (metadata, file_changes)| async move {
                    let oid = metadata.oid;
                    let parents = metadata
                        .parents
                        .iter()
                        .map(|p| {
                            roots
                                .get(&p)
                                .copied()
                                .or_else(|| import_map.get(&p).map(|p| p.0))
                                .ok_or_else(|| format_err!("Commit was not imported: {}", p))
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .with_context(|| format_err!("While looking for parents of {}", oid))?;

                    let bcs =
                        import_bonsai_changeset(ctx, repo, metadata, parents, file_changes, &prefs)
                            .await?;

                    let bcs_id = bcs.get_changeset_id();

                    import_map.insert(oid, (bcs_id, bcs));

                    info!(
                        ctx.logger(),
                        "GitRepo:{} commit {} of {} - Oid:{} => Bid:{}",
                        repo_name_ref,
                        import_map.len(),
                        nb_commits_to_import,
                        oid_to_sha1(&oid)?.to_brief(),
                        bcs_id.to_brief()
                    );

                    Result::<_, Error>::Ok(import_map)
                }
            },
        )
        .await?;

    if prefs.derive_trees {
        for (id, (bcs_id, _bcs)) in import_map.iter() {
            let commit = walk_repo.find_commit(*id)?;
            let tree_id = commit.tree()?.id();

            let derived_tree = TreeHandle::derive(&ctx, &repo, *bcs_id).await?;

            let derived_tree_id = Oid::from_bytes(derived_tree.oid().as_ref())?;

            if tree_id != derived_tree_id {
                let e = format_err!(
                    "Invalid tree was derived for {:?}: {:?} (expected {:?})",
                    commit.id(),
                    derived_tree_id,
                    tree_id
                );
                return Err(e);
            }
        }

        info!(ctx.logger(), "{} tree(s) are valid!", import_map.len());
    }

    if prefs.derive_hg {
        let mut hg_manifests: HashMap<ChangesetId, HgManifestId> = HashMap::new();

        for (id, (bcs_id, bcs)) in import_map.iter() {
            let parent_manifests = future::try_join_all(bcs.parents().map({
                let hg_manifests = &hg_manifests;
                move |p| async move {
                    let manifest = if let Some(manifest) = hg_manifests.get(&p) {
                        *manifest
                    } else {
                        repo.get_hg_from_bonsai_changeset(ctx.clone(), p)
                            .await?
                            .load(ctx, repo.blobstore())
                            .await?
                            .manifestid()
                    };
                    Result::<_, Error>::Ok(manifest)
                }
            }))
            .await?;

            let manifest =
                get_manifest_from_bonsai(repo, ctx.clone(), bcs.clone(), parent_manifests).await?;

            hg_manifests.insert(*bcs_id, manifest);

            info!(ctx.logger(), "Hg: {:?}: {:?}", id, manifest);
        }
    }

    Ok(import_map)
}

async fn import_bonsai_changeset(
    ctx: &CoreContext,
    repo: &BlobRepo,
    metadata: CommitMetadata,
    parents: Vec<ChangesetId>,
    file_changes: BTreeMap<MPath, Option<FileChange>>,
    prefs: &GitimportPreferences,
) -> Result<BonsaiChangeset, Error> {
    let CommitMetadata {
        oid,
        message,
        author,
        author_date,
        committer,
        committer_date,
        ..
    } = metadata;

    let mut extra = BTreeMap::new();
    if prefs.hggit_compatibility {
        extra.insert(
            HGGIT_COMMIT_ID_EXTRA.to_string(),
            oid.to_string().into_bytes(),
        );
    }

    // TODO: Should we have further extras?
    let bcs = BonsaiChangesetMut {
        parents,
        author,
        author_date,
        committer: Some(committer),
        committer_date: Some(committer_date),
        message,
        extra,
        file_changes,
    }
    .freeze()?;

    let bcs_id = bcs.get_changeset_id();

    // We now that the commits are in order (this is guaranteed by the Walk), so we
    // can insert them as-is, one by one, without extra dependency / ordering checks.

    save_bonsai_changesets(vec![bcs.clone()], ctx.clone(), repo.clone()).await?;

    if prefs.bonsai_git_mapping {
        repo.bonsai_git_mapping()
            .bulk_add(
                &ctx,
                &[BonsaiGitMappingEntry::new(oid_to_sha1(&oid)?, bcs_id)],
            )
            .await?;
    }

    Ok(bcs)
}

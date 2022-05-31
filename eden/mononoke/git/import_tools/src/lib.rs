/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(try_blocks)]

mod gitimport_objects;
mod gitlfs;

pub use crate::gitimport_objects::{
    convert_time_to_datetime, oid_to_sha1, CommitMetadata, ExtractedCommit, FullRepoImport,
    GitLeaf, GitManifest, GitRangeImport, GitTree, GitimportPreferences, GitimportTarget,
    ImportMissingForCommit,
};
pub use crate::gitlfs::{GitImportLfs, LfsMetaData};

use anyhow::{bail, format_err, Context, Error};
use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobstore::Blobstore;
use bonsai_git_mapping::BonsaiGitMappingEntry;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use derived_data::BonsaiDerived;
use filestore::{self, FilestoreConfig, StoreRequest};
use futures::{future, stream, Stream, StreamExt, TryStreamExt};
use futures_stats::TimedTryFutureExt;
use git2::{ObjectType, Oid, Repository, Sort};
use git_hash::ObjectId;
pub use git_pool::GitPool;
use git_types::TreeHandle;
use linked_hash_map::LinkedHashMap;
use manifest::{bonsai_diff, BonsaiDiffFileChange, StoreLoadable};
use mercurial_derived_data::{get_manifest_from_bonsai, DeriveHgChangeset};
use mercurial_types::HgManifestId;
use mononoke_types::{
    hash, BonsaiChangeset, BonsaiChangesetMut, ChangesetId, ContentMetadata, FileChange, MPath,
};
use slog::{debug, info};
use sorted_vector_map::SortedVectorMap;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tokio::task;

const HGGIT_COMMIT_ID_EXTRA: &str = "convert_revision";

pub fn git2_oid_to_git_hash_objectid(oid: &Oid) -> git_hash::ObjectId {
    oid.as_bytes().into()
}

pub fn git_hash_oid_to_git2_oid(oid: &git_hash::oid) -> Oid {
    Oid::from_bytes(oid.as_bytes()).expect("Just converting OID types should not fail")
}

fn git_store_request(
    ctx: &CoreContext,
    git_id: ObjectId,
    git_bytes: Bytes,
) -> Result<(StoreRequest, impl Stream<Item = Result<Bytes, Error>>), Error> {
    let size = git_bytes.len().try_into()?;
    let git_sha1 =
        hash::RichGitSha1::from_bytes(Bytes::copy_from_slice(git_id.as_bytes()), "blob", size)?;
    let req = StoreRequest::with_git_sha1(size, git_sha1);
    debug!(
        ctx.logger(),
        "Uploading git-blob:{} size:{}",
        git_sha1.sha1().to_brief(),
        size
    );
    Ok((req, stream::once(async move { Ok(git_bytes) })))
}

async fn do_upload<B: Blobstore + Clone + 'static>(
    ctx: &CoreContext,
    blobstore: &B,
    filestore_config: FilestoreConfig,
    pool: GitPool,
    oid: ObjectId,
    path: &MPath,
    lfs: &GitImportLfs,
) -> Result<ContentMetadata, Error> {
    let (git_id, git_bytes) = pool
        .with({
            move |repo| {
                let bytes = {
                    let odb = repo.odb()?;
                    let odb_object = odb.read(git_hash_oid_to_git2_oid(&oid))?;
                    if odb_object.kind() != ObjectType::Blob {
                        bail!("{} is not a blob", oid);
                    }
                    Bytes::copy_from_slice(odb_object.data())
                };
                Result::<_, Error>::Ok((oid, bytes))
            }
        })
        .await?;

    if let Some(lfs_meta) = lfs.is_lfs_file(&git_bytes, git_id.clone()) {
        cloned!(ctx, lfs, blobstore, filestore_config, path);
        Ok(lfs
            .with(
                ctx,
                lfs_meta,
                move |ctx, lfs_meta, req, bstream| async move {
                    info!(
                        ctx.logger(),
                        "Uploading LFS {} sha256:{} size:{}",
                        path,
                        lfs_meta.sha256.to_brief(),
                        lfs_meta.size,
                    );
                    filestore::store(&blobstore, filestore_config, &ctx, &req, bstream).await
                },
            )
            .await?)
    } else {
        let (req, bstream) = git_store_request(ctx, git_id, git_bytes)?;
        Ok(filestore::store(blobstore, filestore_config, ctx, &req, bstream).await?)
    }
}

// TODO: Try to produce copy-info?
async fn find_file_changes<S, B: Blobstore + Clone + 'static>(
    ctx: &CoreContext,
    blobstore: &B,
    filestore_config: &FilestoreConfig,
    pool: GitPool,
    changes: S,
    lfs: &GitImportLfs,
) -> Result<SortedVectorMap<MPath, FileChange>, Error>
where
    S: Stream<Item = Result<BonsaiDiffFileChange<GitLeaf>, Error>>,
{
    changes
        .map_ok(|change| async {
            task::spawn({
                cloned!(pool, ctx, blobstore, filestore_config, lfs);
                async move {
                    match change {
                        BonsaiDiffFileChange::Changed(path, ty, GitLeaf(oid))
                        | BonsaiDiffFileChange::ChangedReusedId(path, ty, GitLeaf(oid)) => {
                            let meta = do_upload(
                                &ctx,
                                &blobstore,
                                filestore_config,
                                pool,
                                oid,
                                &path,
                                &lfs,
                            )
                            .await?;
                            Ok((
                                path,
                                FileChange::tracked(meta.content_id, ty, meta.total_size, None),
                            ))
                        }
                        BonsaiDiffFileChange::Deleted(path) => Ok((path, FileChange::Deletion)),
                    }
                }
            })
            .await?
        })
        .try_buffer_unordered(100)
        .try_collect()
        .await
}

pub trait GitimportAccumulator: Sized {
    fn new() -> Self;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn insert(&mut self, oid: ObjectId, cs_id: ChangesetId, bonsai: &BonsaiChangeset);
    fn get(&self, oid: &git_hash::oid) -> Option<ChangesetId>;
}

struct BufferingGitimportAccumulator {
    inner: LinkedHashMap<ObjectId, (ChangesetId, BonsaiChangeset)>,
}

impl GitimportAccumulator for BufferingGitimportAccumulator {
    fn new() -> Self {
        Self {
            inner: LinkedHashMap::new(),
        }
    }

    fn len(&self) -> usize {
        self.inner.len()
    }

    fn insert(&mut self, oid: ObjectId, cs_id: ChangesetId, bonsai: &BonsaiChangeset) {
        self.inner.insert(oid, (cs_id, bonsai.clone()));
    }

    fn get(&self, oid: &git_hash::oid) -> Option<ChangesetId> {
        self.inner.get(oid).map(|p| p.0)
    }
}

pub async fn gitimport_acc<Acc: GitimportAccumulator>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    path: &Path,
    target: &dyn GitimportTarget,
    prefs: &GitimportPreferences,
) -> Result<Acc, Error> {
    let repo_name = if let Some(name) = &prefs.gitrepo_name {
        String::from(name)
    } else {
        let name_path = if path.ends_with(".git") {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        String::from(name_path.to_string_lossy())
    };

    let walk_repo = Repository::open(&path)?;
    let pool = &GitPool::new(path.to_path_buf())?;

    let mut walk = walk_repo.revwalk()?;
    walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)?;
    target.populate_walk(&walk_repo, &mut walk)?;

    let roots = &target.get_roots()?;
    let nb_commits_to_import = target.get_nb_commits(&walk_repo)?;
    if 0 == nb_commits_to_import {
        info!(ctx.logger(), "Nothing to import for repo {}.", repo_name);
        return Ok(Acc::new());
    }

    let acc = RefCell::new(Acc::new());

    // Kick off a stream that consumes the walk and prepared commits. Then, produce the Bonsais.
    stream::iter(walk)
        .map(|oid| async {
            let oid =
                git2_oid_to_git_hash_objectid(&(oid.with_context(|| "While walking commits")?));
            task::spawn({
                cloned!(ctx, repo, pool, prefs.lfs);
                async move {
                    let ExtractedCommit {
                        metadata,
                        tree,
                        parent_trees,
                    } = ExtractedCommit::new(oid, &pool)
                        .await
                        .with_context(|| format!("While extracting {}", oid))?;

                    let file_changes = find_file_changes(
                        &ctx,
                        repo.blobstore(),
                        &repo.filestore_config(),
                        pool.clone(),
                        bonsai_diff(ctx.clone(), pool, tree, parent_trees),
                        &lfs,
                    )
                    .await?;

                    Result::<_, Error>::Ok((metadata, file_changes))
                }
            })
            .await?
        })
        .buffered(prefs.concurrency)
        .and_then(|(metadata, file_changes)| async {
            let oid = metadata.oid;
            let parents = metadata
                .parents
                .iter()
                .map(|p| {
                    roots
                        .get(p)
                        .copied()
                        .or_else(|| acc.borrow().get(p))
                        .ok_or_else(|| format_err!("Commit was not imported: {}", p))
                })
                .collect::<Result<Vec<_>, _>>()
                .with_context(|| format_err!("While looking for parents of {}", oid))?;
            let bcs = generate_bonsai_changeset(metadata, parents, file_changes, &prefs)?;
            let bcs_id = bcs.get_changeset_id();
            acc.borrow_mut().insert(oid, bcs_id, &bcs);

            let git_sha1 = oid_to_sha1(&oid)?;
            info!(
                ctx.logger(),
                "GitRepo:{} commit {} of {} - Oid:{} => Bid:{}",
                &repo_name,
                acc.borrow().len(),
                nb_commits_to_import,
                git_sha1.to_brief(),
                bcs_id.to_brief()
            );
            Ok((bcs, git_sha1))
        })
        // Chunk togehter into Vec<std::result::Result<(bcs, oid), Error> >
        .chunks(prefs.concurrency)
        // Go from Vec<Result<X,Y>> -> Result<Vec<X>,Y>
        //.then(|v| future::ready(v.into_iter().collect::<Result<Vec<_>, Error>>()))
        .map(|v| v.into_iter().collect::<Result<Vec<_>, Error>>())
        .try_for_each(|v| async {
            task::spawn({
                cloned!(ctx, repo, prefs);
                async move {
                    let oid_to_bcsid = v
                        .iter()
                        .map(|(bcs, git_sha1)| {
                            BonsaiGitMappingEntry::new(*git_sha1, bcs.get_changeset_id())
                        })
                        .collect::<Vec<BonsaiGitMappingEntry>>();
                    let vbcs = v.into_iter().map(|x| x.0).collect();

                    // We know that the commits are in order (this is guaranteed by the Walk), so we
                    // can insert them as-is, one by one, without extra dependency / ordering checks.
                    let (stats, ()) = save_bonsai_changesets(vbcs, ctx.clone(), &repo)
                        .try_timed()
                        .await?;
                    debug!(
                        ctx.logger(),
                        "save_bonsai_changesets for {} commits in {:?}",
                        oid_to_bcsid.len(),
                        stats.completion_time
                    );

                    if prefs.bonsai_git_mapping {
                        repo.bonsai_git_mapping()
                            .bulk_add(&ctx, &oid_to_bcsid)
                            .await?;
                    }
                    Result::<_, Error>::Ok(())
                }
            })
            .await?
        })
        .await?;

    Ok(acc.into_inner())
}

pub async fn gitimport(
    ctx: &CoreContext,
    repo: &BlobRepo,
    path: &Path,
    target: &dyn GitimportTarget,
    prefs: GitimportPreferences,
) -> Result<LinkedHashMap<ObjectId, (ChangesetId, BonsaiChangeset)>, Error> {
    let import_map =
        gitimport_acc::<BufferingGitimportAccumulator>(ctx, repo, path, target, &prefs)
            .await?
            .inner;

    if prefs.derive_trees {
        let git_repo = Repository::open(&path)?;

        for (id, (bcs_id, _bcs)) in import_map.iter() {
            let commit = gitimport_objects::read_commit(&git_repo, id)?;
            let tree_id = hash::GitSha1::from_bytes(commit.tree.as_bytes())?;

            let derived_tree = TreeHandle::derive(&ctx, &repo, *bcs_id).await?;

            if tree_id != derived_tree.oid().sha1() {
                let e = format_err!(
                    "Invalid tree was derived for {:?}: {:?} (expected {:?})",
                    id,
                    derived_tree.oid(),
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
                        repo.derive_hg_changeset(ctx, p)
                            .await?
                            .load(ctx, repo.blobstore())
                            .await?
                            .manifestid()
                    };
                    Result::<_, Error>::Ok(manifest)
                }
            }))
            .await?;

            let manifest = get_manifest_from_bonsai(
                ctx.clone(),
                repo.get_blobstore().boxed(),
                bcs.clone(),
                parent_manifests,
            )
            .await?;

            hg_manifests.insert(*bcs_id, manifest);

            info!(ctx.logger(), "Hg: {:?}: {:?}", id, manifest);
        }
    }

    Ok(import_map)
}

fn generate_bonsai_changeset(
    metadata: CommitMetadata,
    parents: Vec<ChangesetId>,
    file_changes: SortedVectorMap<MPath, FileChange>,
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

    let mut extra = SortedVectorMap::new();
    if prefs.hggit_compatibility {
        extra.insert(
            HGGIT_COMMIT_ID_EXTRA.to_string(),
            oid.to_string().into_bytes(),
        );
    }

    // TODO: Should we have further extras?
    BonsaiChangesetMut {
        parents,
        author,
        author_date,
        committer: Some(committer),
        committer_date: Some(committer_date),
        message,
        extra,
        file_changes,
        is_snapshot: false,
    }
    .freeze()
}

async fn import_bonsai_changeset(
    ctx: &CoreContext,
    repo: &BlobRepo,
    metadata: CommitMetadata,
    parents: Vec<ChangesetId>,
    file_changes: SortedVectorMap<MPath, FileChange>,
    prefs: &GitimportPreferences,
) -> Result<BonsaiChangeset, Error> {
    let oid = metadata.oid;
    let bcs = generate_bonsai_changeset(metadata, parents, file_changes, prefs)?;
    let bcs_id = bcs.get_changeset_id();

    save_bonsai_changesets(vec![bcs.clone()], ctx.clone(), repo).await?;

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

pub async fn import_tree_as_single_bonsai_changeset(
    ctx: &CoreContext,
    repo: &BlobRepo,
    path: &Path,
    git_cs_id: ObjectId,
    prefs: GitimportPreferences,
) -> Result<BonsaiChangeset, Error> {
    let pool = GitPool::new(path.to_path_buf())?;

    let ExtractedCommit { tree, metadata, .. } = ExtractedCommit::new(git_cs_id, &pool)
        .await
        .with_context(|| format!("While extracting {}", git_cs_id))?;

    let file_changes = find_file_changes(
        ctx,
        repo.blobstore(),
        &repo.filestore_config(),
        pool.clone(),
        bonsai_diff(ctx.clone(), pool, tree, HashSet::new()),
        &prefs.lfs,
    )
    .await?;

    let bcs = import_bonsai_changeset(
        ctx,
        repo,
        metadata,
        vec![], // no parents
        file_changes,
        &prefs,
    )
    .await?;

    Ok(bcs)
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::derive_hg_manifest::derive_hg_manifest;
use crate::derive_hg_manifest::derive_simple_hg_manifest_stack_without_copy_info;
use crate::mapping::HgChangesetDeriveOptions;
use crate::mapping::MappedHgChangesetId;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Error;
use async_trait::async_trait;
use blobrepo_common::changed_files::compute_changed_files;
use blobstore::Blobstore;
use blobstore::Loadable;
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationError;
use futures::future;
use futures::future::try_join;
use futures::future::try_join_all;
use futures::stream;
use futures::FutureExt;
use futures::TryStreamExt;
use manifest::ManifestChanges;
use manifest::ManifestOps;
use mercurial_types::blobs::ChangesetMetadata;
use mercurial_types::blobs::ContentBlobMeta;
use mercurial_types::blobs::File;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::blobs::HgChangesetContent;
use mercurial_types::blobs::UploadHgFileContents;
use mercurial_types::blobs::UploadHgFileEntry;
use mercurial_types::blobs::UploadHgNodeHash;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgParents;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types::TrackedFileChange;
use repo_derived_data::RepoDerivedDataRef;
use stats::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

define_stats! {
    prefix = "mononoke.blobrepo";
    get_hg_from_bonsai_changeset: timeseries(Rate, Sum),
    generate_hg_from_bonsai_changeset: timeseries(Rate, Sum),
    generate_hg_from_bonsai_total_latency_ms: histogram(100, 0, 10_000, Average; P 50; P 75; P 90; P 95; P 99),
    generate_hg_from_bonsai_single_latency_ms: histogram(100, 0, 10_000, Average; P 50; P 75; P 90; P 95; P 99),
    generate_hg_from_bonsai_generated_commit_num: histogram(1, 0, 20, Average; P 50; P 75; P 90; P 95; P 99),
}

async fn can_reuse_filenode(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    parent: HgFileNodeId,
    change: &TrackedFileChange,
) -> Result<Option<HgFileNodeId>, Error> {
    let parent_envelope = parent.load(ctx, blobstore).await?;
    let parent_copyfrom_path = File::extract_copied_from(parent_envelope.metadata())?.map(|t| t.0);
    let parent_content_id = parent_envelope.content_id();

    if parent_content_id == change.content_id()
        && change.copy_from().map(|t| &t.0) == parent_copyfrom_path.as_ref()
    {
        Ok(Some(parent))
    } else {
        Ok(None)
    }
}

pub(crate) async fn store_file_change<'a>(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    p1: Option<HgFileNodeId>,
    p2: Option<HgFileNodeId>,
    path: &'a MPath,
    change: &'a TrackedFileChange,
    copy_from: Option<(MPath, HgFileNodeId)>,
) -> Result<(FileType, HgFileNodeId), Error> {
    // If we produced a hg change that has copy info, then the Bonsai should have copy info
    // too. However, we could have Bonsai copy info without having copy info in the hg change
    // if we stripped it out to produce a hg changeset for an Octopus merge and the copy info
    // references a step-parent (i.e. neither p1, not p2).
    if copy_from.is_some() {
        assert!(change.copy_from().is_some());
    }

    // Mercurial has complicated logic of finding file parents, especially
    // if a file was also copied/moved.
    // See mercurial/localrepo.py:_filecommit().
    // Mononoke uses simpler rules, which still produce a usable result

    // Simplify parents. This aims to reduce the amount of work done in reuse, and avoids
    // semantically unhelpful duplicate parents.
    let (p1, p2) = match (p1, p2) {
        (Some(p1), None) => (Some(p1), None),
        (None, Some(p2)) => (Some(p2), None),
        (p1, p2) if p1 == p2 => (p1, None),
        (p1, p2) => (p1, p2),
    };

    // we can reuse same HgFileNodeId if we have a filenode that has
    // same content and copyfrom information
    let maybe_filenode = match (p1, p2) {
        (Some(parent), None) | (None, Some(parent)) => {
            can_reuse_filenode(&ctx, &blobstore, parent, change).await?
        }
        (Some(p1), Some(p2)) => {
            let (reuse_p1, reuse_p2) = try_join(
                can_reuse_filenode(&ctx, &blobstore, p1, change),
                can_reuse_filenode(&ctx, &blobstore, p2, change),
            )
            .await?;
            reuse_p1.or(reuse_p2)
        }
        // No filenode to reuse
        (None, None) => None,
    };

    let filenode_id = match maybe_filenode {
        Some(filenode) => filenode,
        None => {
            let p1 = if p1.is_some()
                && p2.is_none()
                && copy_from.is_some()
                && copy_from.as_ref().map(|c| &c.0) != Some(path)
            {
                // Mercurial special-cases the "file copied over existing file" case, and does not
                // put in a parent in that situation - `hg log` then looks down the copyfrom
                // information instead. This is not the best decision, but we should keep it
                // For example:
                // ```
                // echo 1 > 1 && echo 2 > 2 && hg ci -A -m first
                // hg cp 2 1 --force && hg ci -m second
                // # File '1' has both p1 and copy from.
                // ```
                // In this case, Mercurial and Mononoke both drop the p1 information in the filenode,
                // instead relying on the copyfrom for history.
                None
            } else {
                p1
            };

            let upload_entry = UploadHgFileEntry {
                upload_node_id: UploadHgNodeHash::Generate,
                contents: UploadHgFileContents::ContentUploaded(ContentBlobMeta {
                    id: change.content_id(),
                    size: change.size(),
                    copy_from: copy_from.clone(),
                }),
                p1,
                p2,
            };

            upload_entry.upload(ctx, blobstore, Some(path)).await?
        }
    };

    Ok((change.file_type(), filenode_id))
}

async fn resolve_paths(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    manifest_id: Option<HgManifestId>,
    paths: Vec<MPath>,
) -> Result<HashMap<MPath, HgFileNodeId>, Error> {
    match manifest_id {
        None => Ok(HashMap::new()),
        Some(manifest_id) => {
            let mapping: HashMap<MPath, HgFileNodeId> = manifest_id
                .find_entries(ctx, blobstore, paths)
                .map_ok(|(path, entry)| Some((path?, entry.into_leaf()?.1)))
                .try_filter_map(future::ok)
                .try_collect()
                .await?;
            Ok(mapping)
        }
    }
}

pub async fn get_manifest_from_bonsai(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    bcs: BonsaiChangeset,
    parent_manifests: Vec<HgManifestId>,
) -> Result<HgManifestId, Error> {
    // NOTE: We ignore further parents beyond p1 and p2 for the purposed of tracking copy info
    // or filenode parents. This is because hg supports just 2 parents at most, so we track
    // copy info & filenode parents relative to the first 2 parents, then ignore other parents.

    let (manifest_p1, manifest_p2) = {
        let mut manifests = parent_manifests.iter();
        (manifests.next().copied(), manifests.next().copied())
    };

    let (p1, p2) = {
        let mut parents = bcs.parents();
        let p1 = parents.next();
        let p2 = parents.next();
        (p1, p2)
    };

    let file_changes = bcs
        .file_changes()
        .map(|(path, fc)| {
            Ok((
                path.clone(),
                match fc {
                    FileChange::Change(tc) => Some(tc.clone()),
                    FileChange::Deletion => None,
                    FileChange::UntrackedChange(_) | FileChange::UntrackedDeletion => {
                        bail!("Can't derive manifest for snapshot")
                    }
                },
            ))
        })
        .collect::<Result<Vec<_>, Error>>()?;

    // paths *modified* by changeset or *copied from parents*
    let mut p1_paths = Vec::new();
    let mut p2_paths = Vec::new();
    for (path, file_change) in file_changes.iter() {
        match file_change {
            Some(file_change) => {
                if let Some((copy_path, bcsid)) = file_change.copy_from() {
                    if Some(bcsid) == p1.as_ref() {
                        p1_paths.push(copy_path.clone());
                    }
                    if Some(bcsid) == p2.as_ref() {
                        p2_paths.push(copy_path.clone());
                    }
                };
                p1_paths.push(path.clone());
                p2_paths.push(path.clone());
            }
            None => {}
        }
    }

    // TODO:
    // `derive_manifest` already provides parents for newly created files, so we
    // can remove **all** lookups to files from here, and only leave lookups for
    // files that were copied (i.e bonsai changes that contain `copy_path`)
    let (p1s, p2s) = try_join(
        resolve_paths(ctx.clone(), blobstore.clone(), manifest_p1, p1_paths),
        resolve_paths(ctx.clone(), blobstore.clone(), manifest_p2, p2_paths),
    )
    .await?;

    let file_changes: Vec<_> = file_changes
        .into_iter()
        .map(|(path, file_change)| Ok::<_, Error>((path, file_change)))
        .collect();
    let changes: Vec<_> = stream::iter(file_changes)
        .map_ok({
            cloned!(ctx, blobstore);
            move |(path, file_change)| match file_change {
                None => future::ok((path, None)).left_future(),
                Some(file_change) => {
                    let copy_from = file_change.copy_from().and_then(|(copy_path, bcsid)| {
                        if Some(bcsid) == p1.as_ref() {
                            p1s.get(copy_path).map(|id| (copy_path.clone(), *id))
                        } else if Some(bcsid) == p2.as_ref() {
                            p2s.get(copy_path).map(|id| (copy_path.clone(), *id))
                        } else {
                            None
                        }
                    });
                    let p1 = p1s.get(&path).cloned();
                    let p2 = p2s.get(&path).cloned();
                    cloned!(ctx, blobstore);
                    let spawned = tokio::spawn(async move {
                        let entry = store_file_change(
                            ctx,
                            blobstore,
                            p1,
                            p2,
                            &path,
                            &file_change,
                            copy_from,
                        )
                        .await?;
                        Ok((path, Some(entry)))
                    });
                    async move { spawned.await? }.boxed().right_future()
                }
            }
        })
        .try_buffer_unordered(100)
        .try_collect()
        .await?;

    let manifest_id = derive_hg_manifest(ctx.clone(), blobstore, parent_manifests, changes).await?;

    Ok(manifest_id)
}

pub(crate) async fn derive_from_parents(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    bonsai: BonsaiChangeset,
    parents: Vec<MappedHgChangesetId>,
    options: &HgChangesetDeriveOptions,
) -> Result<MappedHgChangesetId, Error> {
    let parents = {
        borrowed!(ctx);
        try_join_all(
            parents
                .into_iter()
                .map(|id| async move { id.hg_changeset_id().load(ctx, blobstore).await }),
        )
        .await?
    };

    let parent_manifests = parents.iter().map(|p| p.manifestid()).collect();
    let manifest_id = get_manifest_from_bonsai(
        ctx.clone(),
        blobstore.clone(),
        bonsai.clone(),
        parent_manifests,
    )
    .await?;

    let (hg_cs_id, _) =
        generate_hg_changeset(ctx, blobstore, bonsai, manifest_id, parents, options).await?;
    Ok(MappedHgChangesetId::new(hg_cs_id))
}

pub async fn derive_simple_hg_changeset_stack_without_copy_info(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    bonsais: Vec<BonsaiChangeset>,
    parent: Option<MappedHgChangesetId>,
    options: &HgChangesetDeriveOptions,
) -> Result<HashMap<ChangesetId, MappedHgChangesetId>, Error> {
    let parent = match parent {
        Some(parent) => Some(parent.hg_changeset_id().load(ctx, blobstore).await?),
        None => None,
    };

    let file_changes = bonsais
        .iter()
        .map(|bonsai| {
            let per_commit_file_changes: Result<Vec<_>, Error> = bonsai
                .file_changes()
                .map(|(path, fc)| {
                    use FileChange::*;
                    let tracked_file_change = match fc {
                        Change(tracked_file_change) => Some(tracked_file_change.clone()),
                        Deletion => None,
                        UntrackedChange(_) | UntrackedDeletion => {
                            bail!(
                                "unexpected untracked file change while deriving {}",
                                bonsai.get_changeset_id()
                            );
                        }
                    };
                    Ok((path.clone(), tracked_file_change))
                })
                .collect();
            let per_commit_file_changes = per_commit_file_changes?;

            let mf_changes = ManifestChanges {
                cs_id: bonsai.get_changeset_id(),
                changes: per_commit_file_changes,
            };

            Ok(mf_changes)
        })
        .collect::<Result<Vec<_>, Error>>();
    let file_changes = file_changes?;

    let mf_ids = derive_simple_hg_manifest_stack_without_copy_info(
        ctx.clone(),
        blobstore.clone(),
        file_changes,
        parent.clone().map(|p| p.manifestid()),
    )
    .await?;
    let mut parents = parent.into_iter().collect::<Vec<_>>();

    let mut res = HashMap::with_capacity(bonsais.len());
    for bonsai in bonsais {
        let cs_id = bonsai.get_changeset_id();
        let mf_id = mf_ids.get(&cs_id).ok_or_else(|| {
            anyhow!(
                "not found manifest for {} but should have derived it in this function",
                cs_id
            )
        })?;
        let (hg_changeset_id, hg_cs) =
            generate_hg_changeset(ctx, blobstore, bonsai, *mf_id, parents, options).await?;
        res.insert(cs_id, MappedHgChangesetId::new(hg_changeset_id));
        parents = vec![hg_cs];
    }

    Ok(res)
}

async fn generate_hg_changeset(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    bcs: BonsaiChangeset,
    manifest_id: HgManifestId,
    parents: Vec<HgBlobChangeset>,
    options: &HgChangesetDeriveOptions,
) -> Result<(HgChangesetId, HgBlobChangeset), Error> {
    let start_timestamp = Instant::now();

    // NOTE: We're special-casing the first 2 parents here, since that's all Mercurial
    // supports. Producing the Manifest (in get_manifest_from_bonsai) will consider all
    // parents, but everything else is only presented with the first 2 parents, because that's
    // all Mercurial knows about for now. This lets us produce a meaningful Hg changeset for a
    // Bonsai changeset with > 2 parents (which might be one we imported from Git).
    let mut parents = parents.into_iter();
    let p1 = parents.next();
    let p2 = parents.next();

    let p1_hash = p1.as_ref().map(|p1| p1.get_changeset_id());
    let p2_hash = p2.as_ref().map(|p2| p2.get_changeset_id());

    let mf_p1 = p1.map(|p| p.manifestid());
    let mf_p2 = p2.map(|p| p.manifestid());

    let hg_parents = HgParents::new(
        p1_hash.map(|h| h.into_nodehash()),
        p2_hash.map(|h| h.into_nodehash()),
    );

    // Keep a record of any parents for now (i.e. > 2 parents). We'll store those in extras.
    let step_parents = parents;

    let files = compute_changed_files(
        ctx.clone(),
        blobstore.clone(),
        manifest_id.clone(),
        mf_p1,
        mf_p2,
    )
    .await?;

    let mut metadata = ChangesetMetadata {
        user: bcs.author().to_string(),
        time: *bcs.author_date(),
        extra: bcs
            .extra()
            .map(|(k, v)| (k.as_bytes().to_vec(), v.to_vec()))
            .collect(),
        message: bcs.message().to_string(),
    };
    metadata.record_step_parents(step_parents.map(|blob| blob.get_changeset_id()));
    if options.set_committer_field {
        match (bcs.committer(), bcs.committer_date()) {
            (Some(committer), Some(date)) => {
                // Do not record committer if it's the same as author
                if committer != bcs.author() || date != bcs.author_date() {
                    metadata.record_committer(committer, date)?;
                }
            }
            (None, None) => {}
            _ => {
                bail!("invalid committer/committer date in bonsai changeset");
            }
        };
    }

    let content = HgChangesetContent::new_from_parts(hg_parents, manifest_id, metadata, files);
    let cs = HgBlobChangeset::new(content)?;
    let csid = cs.get_changeset_id();

    cs.save(ctx, blobstore).await?;

    STATS::generate_hg_from_bonsai_single_latency_ms
        .add_value(start_timestamp.elapsed().as_millis() as i64);
    STATS::generate_hg_from_bonsai_generated_commit_num.add_value(1);

    Ok((csid, cs))
}

#[async_trait]
pub trait DeriveHgChangeset {
    async fn derive_hg_changeset(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<HgChangesetId, Error>;
}

#[async_trait]
impl<Repo> DeriveHgChangeset for Repo
where
    Repo: RepoDerivedDataRef + Send + Sync,
{
    async fn derive_hg_changeset(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<HgChangesetId, Error> {
        STATS::get_hg_from_bonsai_changeset.add_value(1);
        let start_timestamp = Instant::now();
        let result = match self
            .repo_derived_data()
            .derive::<MappedHgChangesetId>(ctx, cs_id)
            .await
        {
            Ok(id) => Ok(id.hg_changeset_id()),
            Err(err @ DerivationError::Disabled(..)) => Err(err.into()),
            Err(DerivationError::Error(err)) => Err(err),
        };
        STATS::generate_hg_from_bonsai_total_latency_ms
            .add_value(start_timestamp.elapsed().as_millis() as i64);
        result
    }
}

/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{derive_hg_manifest::derive_hg_manifest, mapping::MappedHgChangesetId};
use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_common::changed_files::compute_changed_files;
use blobstore::{Blobstore, Loadable};
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use derived_data::{BonsaiDerived, DeriveError};
use futures::{
    compat::Future01CompatExt,
    future::{self, try_join, try_join_all},
    stream, FutureExt, TryStreamExt,
};
use manifest::ManifestOps;
use mercurial_types::{
    blobs::{
        ChangesetMetadata, ContentBlobMeta, HgBlobChangeset, HgChangesetContent,
        UploadHgFileContents, UploadHgFileEntry, UploadHgNodeHash,
    },
    HgChangesetId, HgFileNodeId, HgManifestId, HgParents,
};
use mononoke_types::{BonsaiChangeset, ChangesetId, FileChange, FileType, MPath};
use stats::prelude::*;
use std::{collections::HashMap, sync::Arc, time::Instant};

define_stats! {
    prefix = "mononoke.blobrepo";
    get_hg_from_bonsai_changeset: timeseries(Rate, Sum),
    generate_hg_from_bonsai_changeset: timeseries(Rate, Sum),
    generate_hg_from_bonsai_total_latency_ms: histogram(100, 0, 10_000, Average; P 50; P 75; P 90; P 95; P 99),
    generate_hg_from_bonsai_single_latency_ms: histogram(100, 0, 10_000, Average; P 50; P 75; P 90; P 95; P 99),
    generate_hg_from_bonsai_generated_commit_num: histogram(1, 0, 20, Average; P 50; P 75; P 90; P 95; P 99),
}

async fn store_file_change(
    repo: &BlobRepo,
    ctx: CoreContext,
    p1: Option<HgFileNodeId>,
    p2: Option<HgFileNodeId>,
    path: &MPath,
    change: &FileChange,
    copy_from: Option<(MPath, HgFileNodeId)>,
) -> Result<(FileType, HgFileNodeId), Error> {
    // If we produced a hg change that has copy info, then the Bonsai should have copy info
    // too. However, we could have Bonsai copy info without having copy info in the hg change
    // if we stripped it out to produce a hg changeset for an Octopus merge and the copy info
    // references a step-parent (i.e. neither p1, not p2).
    if copy_from.is_some() {
        assert!(change.copy_from().is_some());
    }

    // we can reuse same HgFileNodeId if we have only one parent with same
    // file content but different type (Regular|Executable)
    let maybe_entry = match (p1, p2) {
        (Some(parent), None) | (None, Some(parent)) => {
            let parent_envelope = parent.load(&ctx, repo.blobstore()).await?;
            if parent_envelope.content_id() == change.content_id() && change.copy_from().is_none() {
                Some((change.file_type(), parent))
            } else {
                None
            }
        }
        _ => None,
    };

    match maybe_entry {
        Some(entry) => Ok(entry),
        None => {
            // Mercurial has complicated logic of finding file parents, especially
            // if a file was also copied/moved.
            // See mercurial/localrepo.py:_filecommit(). We have to replicate this
            // logic in Mononoke.
            // TODO(stash): T45618931 replicate all the cases from _filecommit()

            let (p1, p2) = if let Some((ref copy_from_path, _)) = copy_from {
                if copy_from_path != path && p1.is_some() && p2.is_none() {
                    // This case can happen if a file existed in it's parent
                    // but it was copied over:
                    // ```
                    // echo 1 > 1 && echo 2 > 2 && hg ci -A -m first
                    // hg cp 2 1 --force && hg ci -m second
                    // # File '1' has both p1 and copy from.
                    // ```
                    // In that case Mercurial discards p1 i.e. `hg log` will
                    // use copy from revision as a parent. Arguably not the best
                    // decision, but we have to keep it.
                    (None, None)
                } else {
                    (p1, p2)
                }
            } else if p1.is_none() {
                (p2, None)
            } else if p2.is_some() {
                let res = blobrepo_common::file_history::check_if_related(
                    ctx.clone(),
                    repo.clone(),
                    p1.unwrap(),
                    p2.unwrap(),
                    path.clone(),
                )
                .compat()
                .await?;

                use blobrepo_common::file_history::FilenodesRelatedResult::*;
                match res {
                    Unrelated => (p1, p2),
                    FirstAncestorOfSecond => (p2, None),
                    SecondAncestorOfFirst => (p1, None),
                }
            } else {
                (p1, p2)
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
                path: path.clone(),
            };
            let (_, upload_fut) = upload_entry.upload(ctx, repo.get_blobstore().boxed())?;
            let (filenode_id, _) = upload_fut.compat().await?;
            Ok((change.file_type(), filenode_id))
        }
    }
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
    repo: &BlobRepo,
    ctx: CoreContext,
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

    // paths *modified* by changeset or *copied from parents*
    let mut p1_paths = Vec::new();
    let mut p2_paths = Vec::new();
    for (path, file_change) in bcs.file_changes() {
        if let Some(file_change) = file_change {
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
    }

    // TODO:
    // `derive_manifest` already provides parents for newly created files, so we
    // can remove **all** lookups to files from here, and only leave lookups for
    // files that were copied (i.e bonsai changes that contain `copy_path`)
    let blobstore = repo.get_blobstore().boxed();
    let (p1s, p2s) = try_join(
        resolve_paths(ctx.clone(), blobstore.clone(), manifest_p1, p1_paths),
        resolve_paths(ctx.clone(), blobstore, manifest_p2, p2_paths),
    )
    .await?;

    let file_changes: Vec<_> = bcs
        .file_changes()
        .map(|(path, file_change)| Ok::<_, Error>((path.clone(), file_change.cloned())))
        .collect();
    let changes: Vec<_> = stream::iter(file_changes)
        .map_ok({
            cloned!(ctx);
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
                    cloned!(ctx, repo);
                    async move {
                        let entry =
                            store_file_change(&repo, ctx, p1, p2, &path, &file_change, copy_from)
                                .await?;
                        Ok((path, Some(entry)))
                    }
                    .boxed()
                    .right_future()
                }
            }
        })
        .try_buffer_unordered(100)
        .try_collect()
        .await?;

    let manifest_id = derive_hg_manifest(
        ctx.clone(),
        repo.get_blobstore().boxed(),
        parent_manifests,
        changes,
    )
    .await?;

    Ok(manifest_id)
}

pub(crate) async fn derive_from_parents(
    ctx: CoreContext,
    repo: BlobRepo,
    bonsai: BonsaiChangeset,
    parents: Vec<MappedHgChangesetId>,
) -> Result<MappedHgChangesetId, Error> {
    let parents = {
        borrowed!(ctx, repo);
        try_join_all(
            parents
                .into_iter()
                .map(|id| async move { id.0.load(ctx, repo.blobstore()).await }),
        )
        .await?
    };
    let hg_cs_id = generate_hg_changeset(repo, ctx, bonsai, parents).await?;
    Ok(MappedHgChangesetId(hg_cs_id))
}

async fn generate_hg_changeset(
    repo: BlobRepo,
    ctx: CoreContext,
    bcs: BonsaiChangeset,
    parents: Vec<HgBlobChangeset>,
) -> Result<HgChangesetId, Error> {
    let start_timestamp = Instant::now();
    let parent_manifests = parents.iter().map(|p| p.manifestid()).collect();

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

    let manifest_id =
        get_manifest_from_bonsai(&repo, ctx.clone(), bcs.clone(), parent_manifests).await?;
    let files = compute_changed_files(ctx.clone(), repo.clone(), manifest_id.clone(), mf_p1, mf_p2)
        .compat()
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

    let content = HgChangesetContent::new_from_parts(hg_parents, manifest_id, metadata, files);
    let cs = HgBlobChangeset::new(content)?;
    let csid = cs.get_changeset_id();

    cs.save(&ctx, repo.blobstore()).await?;

    STATS::generate_hg_from_bonsai_single_latency_ms
        .add_value(start_timestamp.elapsed().as_millis() as i64);
    STATS::generate_hg_from_bonsai_generated_commit_num.add_value(1);

    Ok(csid)
}

pub async fn get_hg_from_bonsai_changeset(
    repo: BlobRepo,
    ctx: CoreContext,
    bcs_id: ChangesetId,
) -> Result<HgChangesetId, Error> {
    STATS::get_hg_from_bonsai_changeset.add_value(1);
    let start_timestmap = Instant::now();
    let result = match MappedHgChangesetId::derive(&ctx, &repo, bcs_id).await {
        Ok(id) => Ok(id.0),
        Err(err) => match err {
            DeriveError::Disabled(..) => Err(err.into()),
            DeriveError::Error(err) => Err(err),
        },
    };
    STATS::generate_hg_from_bonsai_total_latency_ms
        .add_value(start_timestmap.elapsed().as_millis() as i64);
    result
}

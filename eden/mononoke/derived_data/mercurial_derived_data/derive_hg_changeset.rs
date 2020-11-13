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
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use derived_data::{BonsaiDerived, DeriveError};
use futures::{
    compat::Future01CompatExt,
    future::{try_join_all, FutureExt, TryFutureExt},
    TryStreamExt,
};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt as OldFutureExt, StreamExt};
use futures_old::{future, stream, Future, IntoFuture, Stream};
use futures_stats::futures01::Timed;
use manifest::ManifestOps;
use mercurial_types::{
    blobs::{
        ChangesetMetadata, ContentBlobMeta, HgBlobChangeset, HgBlobEntry, HgChangesetContent,
        UploadHgFileContents, UploadHgFileEntry, UploadHgNodeHash,
    },
    HgChangesetId, HgFileNodeId, HgManifestId, HgParents, Type,
};
use mononoke_types::{BonsaiChangeset, ChangesetId, FileChange, MPath};
use stats::prelude::*;
use std::collections::HashMap;
use time_ext::DurationExt;
use tracing::{trace_args, EventId, Traced};

define_stats! {
    prefix = "mononoke.blobrepo";
    get_hg_from_bonsai_changeset: timeseries(Rate, Sum),
    generate_hg_from_bonsai_changeset: timeseries(Rate, Sum),
    generate_hg_from_bonsai_total_latency_ms: histogram(100, 0, 10_000, Average; P 50; P 75; P 90; P 95; P 99),
    generate_hg_from_bonsai_single_latency_ms: histogram(100, 0, 10_000, Average; P 50; P 75; P 90; P 95; P 99),
    generate_hg_from_bonsai_generated_commit_num: histogram(1, 0, 20, Average; P 50; P 75; P 90; P 95; P 99),
}

fn store_file_change(
    repo: &BlobRepo,
    ctx: CoreContext,
    p1: Option<HgFileNodeId>,
    p2: Option<HgFileNodeId>,
    path: &MPath,
    change: &FileChange,
    copy_from: Option<(MPath, HgFileNodeId)>,
) -> impl Future<Item = HgBlobEntry, Error = Error> + Send {
    // If we produced a hg change that has copy info, then the Bonsai should have copy info
    // too. However, we could have Bonsai copy info without having copy info in the hg change
    // if we stripped it out to produce a hg changeset for an Octopus merge and the copy info
    // references a step-parent (i.e. neither p1, not p2).
    if copy_from.is_some() {
        assert!(change.copy_from().is_some());
    }

    // we can reuse same HgFileNodeId if we have only one parent with same
    // file content but different type (Regular|Executable)
    match (p1, p2) {
        (Some(parent), None) | (None, Some(parent)) => {
            let store = repo.get_blobstore().boxed();
            cloned!(ctx, change, path);
            parent
                .load(ctx, &store)
                .compat()
                .from_err()
                .map(move |parent_envelope| {
                    if parent_envelope.content_id() == change.content_id()
                        && change.copy_from().is_none()
                    {
                        Some(HgBlobEntry::new(
                            store,
                            path.basename().clone(),
                            parent.into_nodehash(),
                            Type::File(change.file_type()),
                        ))
                    } else {
                        None
                    }
                })
                .right_future()
        }
        _ => future::ok(None).left_future(),
    }
    .and_then({
        cloned!(path, change, repo);
        move |maybe_entry| match maybe_entry {
            Some(entry) => future::ok(entry).left_future(),
            None => {
                // Mercurial has complicated logic of finding file parents, especially
                // if a file was also copied/moved.
                // See mercurial/localrepo.py:_filecommit(). We have to replicate this
                // logic in Mononoke.
                // TODO(stash): T45618931 replicate all the cases from _filecommit()

                let parents_fut = if let Some((ref copy_from_path, _)) = copy_from {
                    if copy_from_path != &path && p1.is_some() && p2.is_none() {
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
                        future::ok((None, None)).left_future()
                    } else {
                        future::ok((p1, p2)).left_future()
                    }
                } else if p1.is_none() {
                    future::ok((p2, None)).left_future()
                } else if p2.is_some() {
                    blobrepo_common::file_history::check_if_related(
                        ctx.clone(),
                        repo.clone(),
                        p1.unwrap(),
                        p2.unwrap(),
                        path.clone(),
                    )
                    .map(move |res| {
                        use blobrepo_common::file_history::FilenodesRelatedResult::*;

                        match res {
                            Unrelated => (p1, p2),
                            FirstAncestorOfSecond => (p2, None),
                            SecondAncestorOfFirst => (p1, None),
                        }
                    })
                    .right_future()
                } else {
                    future::ok((p1, p2)).left_future()
                };

                parents_fut
                    .and_then({
                        move |(p1, p2)| {
                            let upload_entry = UploadHgFileEntry {
                                upload_node_id: UploadHgNodeHash::Generate,
                                contents: UploadHgFileContents::ContentUploaded(ContentBlobMeta {
                                    id: change.content_id(),
                                    size: change.size(),
                                    copy_from: copy_from.clone(),
                                }),
                                file_type: change.file_type(),
                                p1,
                                p2,
                                path: path.clone(),
                            };
                            match upload_entry.upload(ctx, repo.get_blobstore().boxed()) {
                                Ok((_, upload_fut)) => {
                                    upload_fut.map(move |(entry, _)| entry).left_future()
                                }
                                Err(err) => future::err(err).right_future(),
                            }
                        }
                    })
                    .right_future()
            }
        }
    })
}

pub fn get_manifest_from_bonsai(
    repo: &BlobRepo,
    ctx: CoreContext,
    bcs: BonsaiChangeset,
    parent_manifests: Vec<HgManifestId>,
) -> BoxFuture<HgManifestId, Error> {
    let event_id = EventId::new();

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

    let resolve_paths = {
        cloned!(ctx);
        let blobstore = repo.get_blobstore();
        move |maybe_manifest_id: Option<HgManifestId>, paths| match maybe_manifest_id {
            None => future::ok(HashMap::new()).right_future(),
            Some(manifest_id) => manifest_id
                .find_entries(ctx.clone(), blobstore.clone(), paths)
                .compat()
                .filter_map(|(path, entry)| Some((path?, entry.into_leaf()?.1)))
                .collect_to::<HashMap<MPath, HgFileNodeId>>()
                .left_future(),
        }
    };

    // TODO:
    // `derive_manifest` already provides parents for newly created files, so we
    // can remove **all** lookups to files from here, and only leave lookups for
    // files that were copied (i.e bonsai changes that contain `copy_path`)
    let store_file_changes = (
        resolve_paths(manifest_p1, p1_paths),
        resolve_paths(manifest_p2, p2_paths),
    )
        .into_future()
        .traced_with_id(
            &ctx.trace(),
            "generate_hg_manifest::traverse_parents",
            trace_args! {},
            event_id,
        )
        .and_then({
            cloned!(ctx, repo);
            move |(p1s, p2s)| {
                let file_changes: Vec<_> = bcs
                    .file_changes()
                    .map(|(path, file_change)| (path.clone(), file_change.cloned()))
                    .collect();
                stream::iter_ok(file_changes)
                    .map({
                        cloned!(ctx);
                        move |(path, file_change)| match file_change {
                            None => future::ok((path, None)).left_future(),
                            Some(file_change) => {
                                let copy_from =
                                    file_change.copy_from().and_then(|(copy_path, bcsid)| {
                                        if Some(bcsid) == p1.as_ref() {
                                            p1s.get(copy_path).map(|id| (copy_path.clone(), *id))
                                        } else if Some(bcsid) == p2.as_ref() {
                                            p2s.get(copy_path).map(|id| (copy_path.clone(), *id))
                                        } else {
                                            None
                                        }
                                    });
                                store_file_change(
                                    &repo,
                                    ctx.clone(),
                                    p1s.get(&path).cloned(),
                                    p2s.get(&path).cloned(),
                                    &path,
                                    &file_change,
                                    copy_from,
                                )
                                .map(move |entry| (path, Some(entry)))
                                .right_future()
                            }
                        }
                    })
                    .buffer_unordered(100)
                    .collect()
                    .traced_with_id(
                        &ctx.trace(),
                        "generate_hg_manifest::store_file_changes",
                        trace_args! {},
                        event_id,
                    )
            }
        });

    let create_manifest = {
        cloned!(ctx, repo);
        move |changes| {
            derive_hg_manifest(
                ctx.clone(),
                repo.get_blobstore().boxed(),
                parent_manifests,
                changes,
            )
            .traced_with_id(
                &ctx.trace(),
                "generate_hg_manifest::create_manifest",
                trace_args! {},
                event_id,
            )
        }
    };

    store_file_changes
        .and_then(create_manifest)
        .traced_with_id(
            &ctx.trace(),
            "generate_hg_manifest",
            trace_args! {},
            event_id,
        )
        .boxify()
}

pub(crate) async fn derive_from_parents(
    ctx: CoreContext,
    repo: BlobRepo,
    bonsai: BonsaiChangeset,
    parents: Vec<MappedHgChangesetId>,
) -> Result<MappedHgChangesetId, Error> {
    let bcs_id = bonsai.get_changeset_id();
    let parents = try_join_all(
        parents
            .into_iter()
            .map(|id| id.0.load(ctx.clone(), repo.blobstore())),
    )
    .await?;
    let hg_cs_id = generate_hg_changeset(repo, ctx, bcs_id, bonsai, parents)
        .compat()
        .await?;
    Ok(MappedHgChangesetId(hg_cs_id))
}

fn generate_hg_changeset(
    repo: BlobRepo,
    ctx: CoreContext,
    bcs_id: ChangesetId,
    bcs: BonsaiChangeset,
    parents: Vec<HgBlobChangeset>,
) -> impl Future<Item = HgChangesetId, Error = Error> + Send {
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

    get_manifest_from_bonsai(&repo, ctx.clone(), bcs.clone(), parent_manifests)
        .and_then({
            cloned!(ctx, repo);
            move |manifest_id| {
                compute_changed_files(ctx, repo, manifest_id.clone(), mf_p1, mf_p2)
                    .map(move |files| (manifest_id, files))
            }
        })
        // create changeset
        .and_then({
            cloned!(ctx, repo, bcs);
            move |(manifest_id, files)| {
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

                let content =
                    HgChangesetContent::new_from_parts(hg_parents, manifest_id, metadata, files);
                let cs = try_boxfuture!(HgBlobChangeset::new(content));
                let cs_id = cs.get_changeset_id();

                cs.save(ctx.clone(), repo.get_blobstore())
                    .map(move |_| cs_id)
                    .boxify()
            }
        })
        .traced(
            &ctx.trace(),
            "generate_hg_changeset",
            trace_args! {"changeset" => bcs_id.to_hex().to_string()},
        )
        .timed(move |stats, _| {
            STATS::generate_hg_from_bonsai_single_latency_ms
                .add_value(stats.completion_time.as_millis_unchecked() as i64);
            STATS::generate_hg_from_bonsai_generated_commit_num.add_value(1);
            Ok(())
        })
}

pub fn get_hg_from_bonsai_changeset(
    repo: &BlobRepo,
    ctx: CoreContext,
    bcs_id: ChangesetId,
) -> impl Future<Item = HgChangesetId, Error = Error> + Send {
    STATS::get_hg_from_bonsai_changeset.add_value(1);
    cloned!(repo);
    async move { MappedHgChangesetId::derive(&ctx, &repo, bcs_id).await }
        .boxed()
        .compat()
        .then(|result| match result {
            Ok(id) => Ok(id.0),
            Err(err) => match err {
                DeriveError::Disabled(..) => Err(err.into()),
                DeriveError::Error(err) => Err(err),
            },
        })
        .timed(move |stats, _| {
            STATS::generate_hg_from_bonsai_total_latency_ms
                .add_value(stats.completion_time.as_millis_unchecked() as i64);
            Ok(())
        })
}

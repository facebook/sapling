/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use crate::errors::ErrorKind;
use anyhow::{bail, Error, Result};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bytes::Bytes;
use cloned::cloned;
use context::{CoreContext, PerfCounterType};
use derived_data::BonsaiDerived;
use derived_data_filenodes::FilenodesOnlyPublic;
use filestore::FetchKey;
use futures::{
    future as old_future, future::IntoFuture as OldIntoFuture, stream as old_stream,
    Future as OldFuture, Stream as OldStream,
};
use futures_ext::{BoxFuture as OldBoxFuture, FutureExt as OldFutureExt};
use futures_preview::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future::{self, FutureExt, TryFutureExt},
    stream::{self, StreamExt, TryStreamExt},
};
use futures_util::try_join;
use load_limiter::Metric;
use manifest::{find_intersection_of_diffs, Entry};
use mercurial_bundles::{
    changegroup::CgVersion,
    part_encode::PartEncodeBuilder,
    parts::{self, FilenodeEntry},
};
use mercurial_revlog::{self, RevlogChangeset};
use mercurial_types::{
    blobs::{fetch_manifest_envelope, File},
    FileBytes, HgBlobNode, HgChangesetId, HgFileNodeId, HgManifestId, HgParents, HgPhase, MPath,
    RevFlags, NULL_CSID,
};
use metaconfig_types::LfsParams;
use mononoke_types::{hash::Sha256, ChangesetId};
use phases::Phases;
use reachabilityindex::LeastCommonAncestorsHint;
use revset::DifferenceOfUnionsOfAncestorsNodeStream;
use slog::debug;
use std::{
    collections::{HashMap, HashSet},
    iter::FromIterator,
    sync::Arc,
};

mod errors;

#[derive(PartialEq, Eq)]
pub enum PhasesPart {
    Yes,
    No,
}

pub async fn create_getbundle_response(
    ctx: CoreContext,
    blobrepo: BlobRepo,
    common: Vec<HgChangesetId>,
    heads: Vec<HgChangesetId>,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    return_phases: PhasesPart,
) -> Result<Vec<PartEncodeBuilder>, Error> {
    let return_phases = return_phases == PhasesPart::Yes;
    debug!(ctx.logger(), "Return phases is: {:?}", return_phases);

    let heads_len = heads.len();
    let common: HashSet<_> = common.into_iter().collect();
    let commits_to_send = find_commits_to_send(&ctx, &blobrepo, &common, &heads, &lca_hint);

    let public_derive_filenodes = async {
        // Calculate phases only for heads that will be sent back to client (i.e. only
        // for heads that are not in "common"). Note that this is different from
        // "phases" part below, where we want to return phases for all heads.
        let filtered_heads = heads.iter().filter(|head| !common.contains(&head));
        let phases = prepare_phases(&ctx, &blobrepo, filtered_heads, &blobrepo.get_phases())
            .compat()
            .await?;
        report_draft_commits(&ctx, phases.iter());
        derive_filenodes_for_public_heads(&ctx, &blobrepo, &common, &phases).await
    };

    let (_, commits_to_send) = try_join!(public_derive_filenodes, commits_to_send)?;

    let mut parts = vec![];
    if heads_len != 0 {
        // no heads means bookmark-only pushrebase, and the client
        // does not expect a changegroup part in this case
        let cs_part = create_hg_changeset_part(&ctx, &blobrepo, commits_to_send).await?;
        parts.push(cs_part);
    }

    // Phases part has to be after the changegroup part.
    if return_phases {
        let phases = prepare_phases(&ctx, &blobrepo, heads.iter(), &blobrepo.get_phases())
            .compat()
            .await?;

        parts.push(parts::phases_part(
            ctx.clone(),
            old_stream::iter_ok(phases),
        )?);
    }

    Ok(parts)
}

fn report_draft_commits<'a, I: IntoIterator<Item = &'a (HgChangesetId, HgPhase)>>(
    ctx: &CoreContext,
    commit_phases: I,
) {
    let num_drafts = commit_phases
        .into_iter()
        .filter(|(_, ref phase)| phase == &HgPhase::Draft)
        .count();
    debug!(
        ctx.logger(),
        "Getbundle returning {} draft commits", num_drafts
    );
    ctx.perf_counters()
        .add_to_counter(PerfCounterType::GetbundleNumDrafts, num_drafts as i64);
}

async fn derive_filenodes_for_public_heads(
    ctx: &CoreContext,
    blobrepo: &BlobRepo,
    common_heads: &HashSet<HgChangesetId>,
    phases: &Vec<(HgChangesetId, HgPhase)>,
) -> Result<(), Error> {
    let mut to_derive_filenodes = vec![];
    for (hg_cs_id, phase) in phases {
        if !common_heads.contains(&hg_cs_id) && phase == &HgPhase::Public {
            to_derive_filenodes.push(*hg_cs_id);
        }
    }

    let to_derive_filenodes_bonsai =
        hg_to_bonsai_stream(&ctx, &blobrepo, to_derive_filenodes).await?;
    Ok(stream::iter(to_derive_filenodes_bonsai)
        .map(move |bcs_id| {
            FilenodesOnlyPublic::derive(ctx.clone(), blobrepo.clone(), bcs_id).compat()
        })
        .buffered(100)
        .try_for_each(|_derive| async { Ok(()) })
        .await?)
}

async fn find_commits_to_send(
    ctx: &CoreContext,
    blobrepo: &BlobRepo,
    common: &HashSet<HgChangesetId>,
    heads: &Vec<HgChangesetId>,
    lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
) -> Result<Vec<ChangesetId>, Error> {
    if common.is_empty() {
        bail!("no 'common' heads specified. Pull will be very inefficient. Please use hg clone instead");
    }

    let common_heads: HashSet<_> = HashSet::from_iter(common.iter());

    let heads = hg_to_bonsai_stream(
        &ctx,
        &blobrepo,
        heads
            .iter()
            .filter(|head| !common_heads.contains(head))
            .cloned()
            .collect(),
    );

    let excludes = hg_to_bonsai_stream(
        &ctx,
        &blobrepo,
        common
            .iter()
            .map(|node| node.clone())
            .filter(|node| node.into_nodehash() != NULL_CSID.into_nodehash())
            .collect(),
    );

    let (heads, excludes) = try_join!(heads, excludes)?;

    let changeset_fetcher = blobrepo.get_changeset_fetcher();
    let nodes_to_send = DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
        ctx.clone(),
        &changeset_fetcher,
        lca_hint.clone(),
        heads,
        excludes,
    )
    .collect()
    .compat()
    .await?;

    ctx.session().bump_load(Metric::EgressCommits, 1.0);
    ctx.perf_counters().add_to_counter(
        PerfCounterType::GetbundleNumCommits,
        nodes_to_send.len() as i64,
    );

    Ok(nodes_to_send.into_iter().rev().collect())
}

async fn create_hg_changeset_part(
    ctx: &CoreContext,
    blobrepo: &BlobRepo,
    nodes_to_send: Vec<ChangesetId>,
) -> Result<PartEncodeBuilder> {
    let map_chunk_size = 100;
    let load_buffer_size = 1000;

    let changelogentries = stream::iter(nodes_to_send)
        .chunks(map_chunk_size)
        .then({
            cloned!(ctx, blobrepo);
            move |bonsais| {
                cloned!(ctx, blobrepo);
                async move {
                    let mapping = blobrepo
                        .get_hg_bonsai_mapping(ctx.clone(), bonsais.clone())
                        .compat()
                        .await?
                        .into_iter()
                        .map(|(hg_cs_id, bonsai_cs_id)| (bonsai_cs_id, hg_cs_id))
                        .collect::<HashMap<_, _>>();

                    // We need to preserve ordering of the Bonsais for Mercurial on the client-side.

                    let ordered_mapping = bonsais
                        .into_iter()
                        .map(|bcs_id| {
                            let hg_cs_id = mapping.get(&bcs_id).ok_or_else(|| {
                                anyhow::format_err!("cs_id was missing from mapping: {:?}", bcs_id)
                            })?;
                            Ok((*hg_cs_id, bcs_id))
                        })
                        .collect::<Vec<_>>();

                    Result::<_, Error>::Ok(ordered_mapping)
                }
            }
        })
        .map_ok(|res| stream::iter(res))
        .try_flatten()
        .map({
            cloned!(ctx, blobrepo);
            move |res| {
                cloned!(ctx, blobrepo);
                async move {
                    match res {
                        Ok((hg_cs_id, _bcs_id)) => {
                            let cs = hg_cs_id
                                .load(ctx.clone(), blobrepo.blobstore())
                                .compat()
                                .await?;
                            Ok((hg_cs_id, cs))
                        }
                        Err(e) => Err(e),
                    }
                }
            }
        })
        .buffered(load_buffer_size)
        .and_then(|(hg_cs_id, cs)| async move {
            let node = hg_cs_id.into_nodehash();

            let revlogcs = RevlogChangeset::new_from_parts(
                cs.parents(),
                cs.manifestid(),
                cs.user().into(),
                cs.time().clone(),
                cs.extra().clone(),
                cs.files().into(),
                cs.comments().into(),
            );

            let mut v = Vec::new();
            mercurial_revlog::changeset::serialize_cs(&revlogcs, &mut v)?;

            Ok((
                node,
                HgBlobNode::new(Bytes::from(v), revlogcs.p1(), revlogcs.p2()),
            ))
        })
        .boxed()
        .compat();

    parts::changegroup_part(changelogentries, None, CgVersion::Cg2Version)
}

async fn hg_to_bonsai_stream(
    ctx: &CoreContext,
    repo: &BlobRepo,
    nodes: Vec<HgChangesetId>,
) -> Result<Vec<ChangesetId>, Error> {
    stream::iter(nodes)
        .map({
            move |node| {
                repo.get_bonsai_from_hg(ctx.clone(), node)
                    .and_then(move |maybe_bonsai| {
                        maybe_bonsai.ok_or(ErrorKind::BonsaiNotFoundForHgChangeset(node).into())
                    })
                    .compat()
            }
        })
        .buffered(100)
        .try_collect()
        .await
}

/// Calculate phases for the heads.
/// If client is pulling non-public changesets phases for public roots should be included.
fn prepare_phases<'a>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    heads: impl IntoIterator<Item = &'a HgChangesetId>,
    phases: &Arc<dyn Phases>,
) -> impl OldFuture<Item = Vec<(HgChangesetId, HgPhase)>, Error = Error> {
    // create 'bonsai changesetid' => 'hg changesetid' hash map that will be later used
    // heads that are not known by the server will be skipped
    let heads: Vec<_> = heads.into_iter().cloned().collect();
    repo.get_hg_bonsai_mapping(ctx.clone(), heads)
        .map(move |hg_bonsai_mapping| {
            hg_bonsai_mapping
                .into_iter()
                .map(|(hg_cs_id, bonsai)| (bonsai, hg_cs_id))
                .collect::<HashMap<ChangesetId, HgChangesetId>>()
        })
        .and_then({
            // calculate phases for the heads
            cloned!(ctx, phases);
            move |bonsai_node_mapping| {
                phases
                    .get_public(ctx, bonsai_node_mapping.keys().cloned().collect(), false)
                    .map(move |public| (public, bonsai_node_mapping))
            }
        })
        .and_then({
            cloned!(ctx, repo, phases);
            move |(public, bonsai_node_mapping)| {
                // select draft heads
                let drafts = bonsai_node_mapping
                    .keys()
                    .filter(|csid| !public.contains(csid))
                    .cloned()
                    .collect();

                // find the public roots for the draft heads
                calculate_public_roots(ctx.clone(), repo.clone(), drafts, phases)
                    .and_then({
                        cloned!(ctx);
                        move |bonsais| {
                            repo.get_hg_bonsai_mapping(ctx, bonsais.into_iter().collect::<Vec<_>>())
                        }
                    })
                    .map(move |public_roots| {
                        let phases = bonsai_node_mapping
                            .into_iter()
                            .map(move |(csid, hg_csid)| {
                                let phase = if public.contains(&csid) {
                                    HgPhase::Public
                                } else {
                                    HgPhase::Draft
                                };
                                (hg_csid, phase)
                            })
                            .chain(
                                public_roots
                                    .into_iter()
                                    .map(|(hg_csid, _)| (hg_csid, HgPhase::Public)),
                            )
                            .collect();
                        phases
                    })
            }
        })
}

/// Calculate public roots for the set of draft changesets
fn calculate_public_roots(
    ctx: CoreContext,
    repo: BlobRepo,
    drafts: HashSet<ChangesetId>,
    phases: Arc<dyn Phases>,
) -> impl OldFuture<Item = HashSet<ChangesetId>, Error = Error> {
    old_future::loop_fn(
        (drafts, HashSet::new(), HashSet::new()),
        move |(drafts, mut public, mut visited)| {
            if drafts.is_empty() {
                return old_future::ok(old_future::Loop::Break(public)).left_future();
            }

            old_stream::iter_ok(drafts)
                .map({
                    cloned!(repo, ctx);
                    move |csid| repo.get_changeset_parents_by_bonsai(ctx.clone(), csid)
                })
                .buffered(100)
                .collect()
                .map(move |parents| {
                    let parents: HashSet<_> = parents
                        .into_iter()
                        .flatten()
                        .filter(|csid| !visited.contains(csid))
                        .collect();
                    visited.extend(parents.iter().cloned());
                    (parents, visited)
                })
                .and_then({
                    cloned!(ctx, phases);
                    move |(parents, visited)| {
                        phases
                            .get_public(ctx, parents.iter().cloned().collect(), false)
                            .map(move |public_phases| (public_phases, parents, visited))
                    }
                })
                .and_then(|(public_phases, parents, visited)| {
                    // split by phase
                    let (new_public, new_drafts) = parents
                        .into_iter()
                        .partition(|csid| public_phases.contains(csid));
                    // update found public changests
                    public.extend(new_public);
                    // continue for the new drafts
                    old_future::ok(old_future::Loop::Continue((new_drafts, public, visited)))
                })
                .right_future()
        },
    )
}

pub enum FilenodeEntryContent {
    InlineV2(FileBytes),
    InlineV3(FileBytes),
    LfsV3(Sha256, u64),
}

pub struct PreparedFilenodeEntry {
    pub filenode: HgFileNodeId,
    pub linknode: HgChangesetId,
    pub parents: HgParents,
    pub metadata: Bytes,
    pub content: FilenodeEntryContent,
}

impl PreparedFilenodeEntry {
    fn into_filenode(
        self,
    ) -> Result<(HgFileNodeId, HgChangesetId, HgBlobNode, Option<RevFlags>), Error> {
        let Self {
            filenode,
            linknode,
            parents,
            metadata,
            content,
        } = self;

        let (blob, flags) = match content {
            FilenodeEntryContent::InlineV2(bytes) => {
                (generate_inline_file(&bytes, parents, &metadata), None)
            }
            FilenodeEntryContent::InlineV3(bytes) => (
                generate_inline_file(&bytes, parents, &metadata),
                Some(RevFlags::REVIDX_DEFAULT_FLAGS),
            ),
            FilenodeEntryContent::LfsV3(oid, size) => (
                generate_lfs_file(oid, parents, size, &metadata)?,
                Some(RevFlags::REVIDX_EXTSTORED),
            ),
        };

        Ok((filenode, linknode, blob, flags))
    }
}

fn prepare_filenode_entries_stream(
    ctx: CoreContext,
    repo: BlobRepo,
    filenodes: Vec<(MPath, HgFileNodeId, HgChangesetId)>,
    lfs_params: LfsParams,
) -> impl OldStream<Item = (MPath, Vec<PreparedFilenodeEntry>), Error = Error> {
    old_stream::iter_ok(filenodes.into_iter())
        .map({
            cloned!(ctx, repo);
            move |(path, filenode, linknode)| {
                filenode
                    .load(ctx.clone(), repo.blobstore())
                    .from_err()
                    .and_then({
                        cloned!(ctx, lfs_params, repo);
                        move |envelope| {
                            let file_size = envelope.content_size();
                            let content = filestore::fetch_stream(
                                repo.blobstore(),
                                ctx.clone(),
                                envelope.content_id(),
                            )
                            .concat2()
                            .map(FileBytes);

                            let content = match lfs_params.threshold {
                                None => content.map(FilenodeEntryContent::InlineV2).boxify(),
                                Some(lfs_threshold) if file_size <= lfs_threshold => {
                                    content.map(FilenodeEntryContent::InlineV3).boxify()
                                }
                                _ => {
                                    let key = FetchKey::from(envelope.content_id());
                                    filestore::get_metadata(repo.blobstore(), ctx.clone(), &key)
                                        .and_then(move |meta| {
                                            let meta = meta.ok_or_else(|| {
                                                Error::from(ErrorKind::MissingContent(key))
                                            })?;
                                            Ok(meta.sha256)
                                        })
                                        .map(move |oid| FilenodeEntryContent::LfsV3(oid, file_size))
                                        .boxify()
                                }
                            };

                            let parents = filenode
                                .load(ctx, repo.blobstore())
                                .from_err()
                                .map(|envelope| envelope.hg_parents());

                            (content, parents)
                                .into_future()
                                .map(move |(content, parents)| PreparedFilenodeEntry {
                                    filenode,
                                    linknode,
                                    parents,
                                    metadata: envelope.metadata().clone(),
                                    content,
                                })
                                .map(move |entry| (path, vec![entry]))
                        }
                    })
            }
        })
        .buffered(100)
}

fn generate_inline_file(content: &FileBytes, parents: HgParents, metadata: &Bytes) -> HgBlobNode {
    let mut parents = parents.into_iter();
    let p1 = parents.next();
    let p2 = parents.next();

    // Metadata is only used to store copy/rename information
    let no_rename_metadata = metadata.is_empty();
    let mut res = vec![];
    res.extend(metadata);
    res.extend(content.as_bytes());
    if no_rename_metadata {
        HgBlobNode::new(Bytes::from(res), p1, p2)
    } else {
        // Mercurial has a complicated logic regarding storing renames
        // If copy/rename metadata is stored then p1 is always "null"
        // (i.e. hash like "00000000....") - that's why we set it to None below.
        // p2 is null for a non-merge commit, but not-null for merges.
        // (See D6922881 for more details about merge logic)
        //
        // It boils down to the fact that we can't have both p1 and p2 to be
        // non-null if we have rename metadata.
        // `HgFileEnvelope::hg_parents()` returns HgParents structure, which
        // always makes p2 a null commit if at least one parent commit is null.
        // And that's why we set the second parent to p1 below.
        debug_assert!(p2.is_none());
        HgBlobNode::new(Bytes::from(res), None, p1)
    }
}

fn generate_lfs_file(
    oid: Sha256,
    parents: HgParents,
    file_size: u64,
    metadata: &Bytes,
) -> Result<HgBlobNode, Error> {
    let copy_from = File::extract_copied_from(metadata)?;
    let bytes = File::generate_lfs_file(oid, file_size, copy_from)?;

    let mut parents = parents.into_iter();
    let p1 = parents.next();
    let p2 = parents.next();
    Ok(HgBlobNode::new(Bytes::from(bytes), p1, p2))
}

pub fn create_manifest_entries_stream(
    ctx: CoreContext,
    repo: BlobRepo,
    manifests: Vec<(Option<MPath>, HgManifestId, HgChangesetId)>,
) -> impl OldStream<Item = OldBoxFuture<parts::TreepackPartInput, Error>, Error = Error> {
    old_stream::iter_ok(manifests.into_iter()).map({
        cloned!(ctx, repo);
        move |(fullpath, mf_id, linknode)| {
            fetch_manifest_envelope(ctx.clone(), &repo.get_blobstore().boxed(), mf_id)
                .map(move |mf_envelope| {
                    let (p1, p2) = mf_envelope.parents();
                    parts::TreepackPartInput {
                        node: mf_id.into_nodehash(),
                        p1,
                        p2,
                        content: mf_envelope.contents().clone(),
                        fullpath,
                        linknode: linknode.into_nodehash(),
                    }
                })
                .boxify()
        }
    })
}

async fn diff_with_parents(
    ctx: CoreContext,
    repo: BlobRepo,
    hg_cs_id: HgChangesetId,
) -> Result<
    (
        Vec<(Option<MPath>, HgManifestId, HgChangesetId)>,
        Vec<(MPath, HgFileNodeId, HgChangesetId)>,
    ),
    Error,
> {
    let (mf_id, parent_mf_ids) = try_join!(fetch_manifest(ctx.clone(), &repo, &hg_cs_id), async {
        let parents = repo
            .get_changeset_parents(ctx.clone(), hg_cs_id)
            .compat()
            .await?;

        future::try_join_all(
            parents
                .iter()
                .map(|p| fetch_manifest(ctx.clone(), &repo, p)),
        )
        .await
    })?;

    let blobstore = Arc::new(repo.get_blobstore());
    let new_entries: Vec<(Option<MPath>, Entry<_, _>)> =
        find_intersection_of_diffs(ctx, blobstore, mf_id, parent_mf_ids)
            .compat()
            .try_collect()
            .await?;

    let mut mfs = vec![];
    let mut files = vec![];
    for (path, entry) in new_entries {
        match entry {
            Entry::Tree(mf) => {
                mfs.push((path, mf, hg_cs_id.clone()));
            }
            Entry::Leaf((_, file)) => {
                let path = path.expect("empty file paths?");
                files.push((path, file, hg_cs_id.clone()));
            }
        }
    }

    Ok((mfs, files))
}

pub fn create_filenodes(
    entries: HashMap<MPath, Vec<PreparedFilenodeEntry>>,
) -> Result<Vec<(MPath, Vec<FilenodeEntry>)>, Error> {
    entries
        .into_iter()
        .map(|(path, prepared_entries)| {
            let entries: Result<Vec<_>, Error> = prepared_entries
                .into_iter()
                .map(PreparedFilenodeEntry::into_filenode)
                .collect();
            Ok((path, entries?))
        })
        .collect()
}

pub fn get_manifests_and_filenodes(
    ctx: CoreContext,
    repo: BlobRepo,
    commits: Vec<HgChangesetId>,
    lfs_params: LfsParams,
) -> impl OldFuture<
    Item = (
        Vec<OldBoxFuture<parts::TreepackPartInput, Error>>,
        HashMap<MPath, Vec<PreparedFilenodeEntry>>,
    ),
    Error = Error,
> {
    old_stream::iter_ok(commits)
        .and_then({
            cloned!(ctx, lfs_params, repo);
            move |hg_cs_id| {
                diff_with_parents(ctx.clone(), repo.clone(), hg_cs_id)
                    .boxed()
                    .compat()
                    .and_then({
                        cloned!(ctx, lfs_params, repo);
                        move |(manifests, filenodes)| {
                            (
                                create_manifest_entries_stream(
                                    ctx.clone(),
                                    repo.clone(),
                                    manifests,
                                )
                                .collect(),
                                prepare_filenode_entries_stream(
                                    ctx,
                                    repo,
                                    filenodes,
                                    lfs_params.clone(),
                                )
                                .collect(),
                            )
                        }
                    })
            }
        })
        .collect()
        .map(move |entries| {
            let mut all_mf_entries = vec![];
            let mut all_filenode_entries: HashMap<_, Vec<_>> = HashMap::new();
            for (mf_entries, file_entries) in entries {
                all_mf_entries.extend(mf_entries);
                for (file_path, filenodes) in file_entries {
                    all_filenode_entries
                        .entry(file_path)
                        .or_default()
                        .extend(filenodes);
                }
            }

            (all_mf_entries, all_filenode_entries)
        })
}

async fn fetch_manifest(
    ctx: CoreContext,
    repo: &BlobRepo,
    hg_cs_id: &HgChangesetId,
) -> Result<HgManifestId, Error> {
    let blob_cs = hg_cs_id.load(ctx, repo.blobstore()).compat().await?;
    Ok(blob_cs.manifestid())
}

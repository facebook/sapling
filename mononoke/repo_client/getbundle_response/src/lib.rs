/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use crate::errors::ErrorKind;
use anyhow::{bail, Error, Result};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bytes::Bytes;
use cloned::cloned;
use context::{CoreContext, Metric, PerfCounterType};
use derived_data::BonsaiDerived;
use derived_data_filenodes::{FilenodesOnlyPublic, FilenodesOnlyPublicMapping};
use futures::{future, stream as old_stream, Future, Stream as OldStream};
use futures_ext::FutureExt as OldFutureExt;
use futures_preview::{compat::Future01CompatExt, stream, StreamExt, TryStreamExt};
use futures_util::try_join;
use mercurial_bundles::{changegroup::CgVersion, part_encode::PartEncodeBuilder, parts};
use mercurial_revlog::{self, RevlogChangeset};
use mercurial_types::{HgBlobNode, HgChangesetId, HgPhase, NULL_CSID};
use mononoke_types::ChangesetId;
use phases::Phases;
use reachabilityindex::LeastCommonAncestorsHint;
use revset::DifferenceOfUnionsOfAncestorsNodeStream;
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
    stream::iter(to_derive_filenodes_bonsai)
        .map(move |bcs_id| {
            FilenodesOnlyPublic::derive(
                ctx.clone(),
                blobrepo.clone(),
                FilenodesOnlyPublicMapping::new(blobrepo.clone()),
                bcs_id,
            )
            .compat()
        })
        .buffered(100)
        .try_for_each(|_derive| async { Ok(()) })
        .await
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
    let changesets_buffer_size = 1000;

    let changelogentries = old_stream::iter_ok(nodes_to_send)
        .map({
            cloned!(blobrepo, ctx);
            move |bonsai| {
                blobrepo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bonsai)
                    .and_then({
                        cloned!(ctx, blobrepo);
                        move |node| {
                            node.load(ctx, blobrepo.blobstore())
                                .from_err()
                                .map(move |cs| (node.into_nodehash(), cs))
                        }
                    })
            }
        })
        .buffered(changesets_buffer_size)
        .and_then(|(node, cs)| {
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
        });

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
) -> impl Future<Item = Vec<(HgChangesetId, HgPhase)>, Error = Error> {
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
                    .get_public(ctx, bonsai_node_mapping.keys().cloned().collect())
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
) -> impl Future<Item = HashSet<ChangesetId>, Error = Error> {
    future::loop_fn(
        (drafts, HashSet::new(), HashSet::new()),
        move |(drafts, mut public, mut visited)| {
            if drafts.is_empty() {
                return future::ok(future::Loop::Break(public)).left_future();
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
                            .get_public(ctx, parents.iter().cloned().collect())
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
                    future::ok(future::Loop::Continue((new_drafts, public, visited)))
                })
                .right_future()
        },
    )
}

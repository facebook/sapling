/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::errors::ErrorKind;
use anyhow::{bail, Error, Result};
use blobrepo::BlobRepo;
use bytes::Bytes;
use cloned::cloned;
use context::{CoreContext, Metric, PerfCounterType};
use futures::{future, stream, Future, Stream};
use futures_ext::FutureExt;
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

pub fn create_getbundle_response(
    ctx: CoreContext,
    blobrepo: BlobRepo,
    common: Vec<HgChangesetId>,
    heads: Vec<HgChangesetId>,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    phases_hint: Option<Arc<dyn Phases>>,
) -> Result<Vec<PartEncodeBuilder>> {
    if common.is_empty() {
        bail!("no 'common' heads specified. Pull will be very inefficient. Please use hg clone instead");
    }

    let changesets_buffer_size = 1000; // TODO(stash): make it configurable
    let heads_len = heads.len();

    let phases_part = if let Some(phases_hint) = phases_hint {
        // Phases were requested
        Some(parts::phases_part(
            ctx.clone(),
            prepare_phases_stream(ctx.clone(), blobrepo.clone(), heads.clone(), phases_hint),
        ))
    } else {
        None
    };

    let blobrepo = Arc::new(blobrepo.clone());
    let common_heads: HashSet<_> = HashSet::from_iter(common.iter());

    let heads = hg_to_bonsai_stream(
        ctx.clone(),
        &blobrepo,
        heads
            .iter()
            .filter(|head| !common_heads.contains(head))
            .cloned()
            .collect(),
    );

    let excludes = hg_to_bonsai_stream(
        ctx.clone(),
        &blobrepo,
        common
            .iter()
            .map(|node| node.clone())
            .filter(|node| node.into_nodehash() != NULL_CSID.into_nodehash())
            .collect(),
    );

    let changeset_fetcher = blobrepo.get_changeset_fetcher();
    let nodes_to_send = heads
        .join(excludes)
        .map({
            cloned!(ctx);
            move |(heads, excludes)| {
                DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
                    ctx,
                    &changeset_fetcher,
                    lca_hint,
                    heads,
                    excludes,
                )
            }
        })
        .flatten_stream();

    // TODO(stash): avoid collecting all the changelogs in the vector - T25767311
    let nodes_to_send = nodes_to_send
        .collect()
        .inspect({
            cloned!(ctx);
            move |nodes| {
                ctx.session().bump_load(Metric::EgressCommits, 1.0);
                ctx.perf_counters()
                    .add_to_counter(PerfCounterType::GetbundleNumCommits, nodes.len() as i64);
            }
        })
        .map(|nodes| stream::iter_ok(nodes.into_iter().rev()))
        .flatten_stream();

    let changelogentries = nodes_to_send
        .map({
            cloned!(blobrepo);
            move |bonsai| {
                blobrepo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bonsai)
                    .map(|cs| cs.into_nodehash())
                    .and_then({
                        cloned!(ctx, blobrepo);
                        move |node| {
                            blobrepo
                                .get_changeset_by_changesetid(ctx, HgChangesetId::new(node))
                                .map(move |cs| (node, cs))
                        }
                    })
            }
        })
        .buffered(changesets_buffer_size)
        .and_then(|(node, cs)| {
            let revlogcs = RevlogChangeset::new_from_parts(
                cs.parents().clone(),
                cs.manifestid().clone(),
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

    let mut parts = vec![];
    if heads_len != 0 {
        // no heads means bookmark-only pushrebase, and the client
        // does not expect a changegroup part in this case
        parts.push(parts::changegroup_part(
            changelogentries,
            None,
            CgVersion::Cg2Version,
        ));
    }

    // Phases part has to be after the changegroup part.
    if let Some(phases_part) = phases_part {
        parts.push(phases_part);
    }

    parts.into_iter().collect::<Result<Vec<_>>>()
}

fn hg_to_bonsai_stream(
    ctx: CoreContext,
    repo: &Arc<BlobRepo>,
    nodes: Vec<HgChangesetId>,
) -> impl Future<Item = Vec<ChangesetId>, Error = Error> {
    stream::iter_ok(nodes.into_iter())
        .map({
            cloned!(repo);
            move |node| {
                repo.get_bonsai_from_hg(ctx.clone(), node)
                    .and_then(move |maybe_bonsai| {
                        maybe_bonsai.ok_or(ErrorKind::BonsaiNotFoundForHgChangeset(node).into())
                    })
            }
        })
        .buffered(100)
        .collect()
}

/// Calculate phases for the heads.
/// If client is pulling non-public changesets phases for public roots should be included.
fn prepare_phases_stream(
    ctx: CoreContext,
    repo: BlobRepo,
    heads: Vec<HgChangesetId>,
    phases: Arc<dyn Phases>,
) -> impl Stream<Item = (HgChangesetId, HgPhase), Error = Error> {
    // create 'bonsai changesetid' => 'hg changesetid' hash map that will be later used
    // heads that are not known by the server will be skipped
    repo.get_hg_bonsai_mapping(ctx.clone(), heads)
        .map(move |hg_bonsai_mapping| {
            hg_bonsai_mapping
                .into_iter()
                .map(|(hg_cs_id, bonsai)| (bonsai, hg_cs_id))
                .collect::<HashMap<ChangesetId, HgChangesetId>>()
        })
        .and_then({
            // calculate phases for the heads
            cloned!(ctx, repo, phases);
            move |bonsai_node_mapping| {
                phases
                    .get_public(ctx, repo, bonsai_node_mapping.keys().cloned().collect())
                    .map(move |public| (public, bonsai_node_mapping))
            }
        })
        .and_then(move |(public, bonsai_node_mapping)| {
            // select draft heads
            let drafts = bonsai_node_mapping
                .keys()
                .filter(|csid| !public.contains(csid))
                .cloned()
                .collect();

            // find the public roots for the draft heads
            calculate_public_roots(ctx.clone(), repo.clone(), drafts, phases)
                .and_then(move |bonsais| {
                    repo.get_hg_bonsai_mapping(ctx, bonsais.into_iter().collect::<Vec<_>>())
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
                        );
                    stream::iter_ok(phases)
                })
        })
        .flatten_stream()
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

            stream::iter_ok(drafts)
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
                    cloned!(ctx, repo, phases);
                    move |(parents, visited)| {
                        phases
                            .get_public(ctx, repo, parents.iter().cloned().collect())
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

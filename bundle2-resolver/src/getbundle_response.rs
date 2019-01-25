// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;
use errors::*;
use failure::err_msg;
use std::collections::{HashMap, HashSet};
use std::iter::FromIterator;
use std::sync::Arc;

use blobrepo::BlobRepo;
use context::CoreContext;
use futures::{future, stream, Future, Stream};
use futures_ext::{BoxFuture, FutureExt};
use mercurial::{self, RevlogChangeset};
use mercurial_bundles::{part_encode::PartEncodeBuilder, parts};
use mercurial_types::{Changeset, HgBlobNode, HgChangesetId, HgNodeHash, HgPhase, NULL_CSID};
use phases::{Phase, Phases};
use reachabilityindex::LeastCommonAncestorsHint;
use revset::DifferenceOfUnionsOfAncestorsNodeStream;

use mononoke_types::ChangesetId;

pub fn create_getbundle_response(
    ctx: CoreContext,
    blobrepo: BlobRepo,
    common: Vec<HgChangesetId>,
    heads: Vec<HgChangesetId>,
    lca_hint: Arc<LeastCommonAncestorsHint>,
    phases_hint: Option<Arc<Phases>>,
) -> Result<Vec<PartEncodeBuilder>> {
    if common.is_empty() {
        return Err(err_msg("no 'common' heads specified. Pull will be very inefficient. Please use hg clone instead"));
    }

    let changesets_buffer_size = 1000; // TODO(stash): make it configurable

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
                ctx.perf_counters()
                    .add_to_counter("getbundle_num_commits", nodes.len() as i64);
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
                                .get_changeset_by_changesetid(ctx, &HgChangesetId::new(node))
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
            mercurial::changeset::serialize_cs(&revlogcs, &mut v)?;
            Ok((
                node,
                HgBlobNode::new(Bytes::from(v), revlogcs.p1(), revlogcs.p2()),
            ))
        });

    let mut parts = vec![];

    parts.push(parts::changegroup_part(changelogentries));

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
                repo.get_bonsai_from_hg(ctx.clone(), &node)
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
    phases_hint: Arc<Phases>,
) -> impl Stream<Item = (HgNodeHash, HgPhase), Error = Error> {
    // create 'bonsai changesetid' => 'hg changesetid' hash map that will be later used
    // heads that are not known by the server will be skipped
    let mapping_fut =
        repo.get_hg_bonsai_mapping(ctx.clone(), heads)
            .map(move |hg_bonsai_mapping| {
                hg_bonsai_mapping
                    .into_iter()
                    .map(|(hg_cs_id, bonsai)| (bonsai, hg_cs_id))
                    .collect::<HashMap<ChangesetId, HgChangesetId>>()
            });

    // calculate phases for the heads
    let heads_phases_fut = mapping_fut.and_then({
        cloned!(ctx, repo, phases_hint);
        move |bonsai_node_mapping| {
            phases_hint
                .get_all(ctx, repo, bonsai_node_mapping.keys().cloned().collect())
                .map(move |phases_mapping| (phases_mapping, bonsai_node_mapping))
        }
    });

    // calculate public roots if client is pulling non-public changesets
    // and join the result together
    heads_phases_fut
        .and_then(move |(phases_mapping, bonsai_node_mapping)| {
            let heads_phases = phases_mapping.calculated;
            let maybe_public_heads = phases_mapping.maybe_public_heads;

            // select draft heads
            let drafts = heads_phases
                .iter()
                .filter_map(|(cs_id, phase)| {
                    if phase == &Phase::Draft {
                        Some(cs_id.clone())
                    } else {
                        None
                    }
                })
                .collect();

            // find the public roots for the draft heads
            let pub_roots_future = calculate_public_roots(
                ctx.clone(),
                repo.clone(),
                drafts,
                phases_hint,
                maybe_public_heads,
            )
            .and_then(move |bonsais| {
                repo.get_hg_bonsai_mapping(ctx, bonsais.into_iter().collect::<Vec<_>>())
            });

            // merge the phases for heads and public roots together and transform the result in a format used by the encoding function
            pub_roots_future.map(move |public_roots| {
                let phases = heads_phases
                    .into_iter()
                    .map(|(bonsai, phase)| {
                        (
                            bonsai_node_mapping[&bonsai].into_nodehash(),
                            HgPhase::from(phase),
                        )
                    })
                    .chain(
                        public_roots
                            .into_iter()
                            .map(|(hg_cs_id, _)| (hg_cs_id.into_nodehash(), HgPhase::Public)),
                    )
                    .collect::<Vec<_>>();

                stream::iter_ok(phases.into_iter())
            })
        })
        .flatten_stream()
}

/// Calculate public roots for the set of draft changesets
fn calculate_public_roots(
    ctx: CoreContext,
    repo: BlobRepo,
    drafts: HashSet<ChangesetId>,
    phases_hint: Arc<Phases>,
    maybe_public_heads: Option<Arc<HashSet<ChangesetId>>>,
) -> BoxFuture<HashSet<ChangesetId>, Error> {
    future::loop_fn(
        (maybe_public_heads, drafts, HashSet::new(), HashSet::new()),
        move |(maybe_public_heads, drafts, mut public, mut visited)| {
            // just precaution: if there are no public heads in the repo at all
            let no_public_heads = if let Some(ref public_heads) = maybe_public_heads {
                public_heads.is_empty()
            } else {
                false
            };
            // nothing more to calculate
            if drafts.is_empty() || no_public_heads {
                return future::ok(future::Loop::Break(public)).left_future();
            }

            // calculate parents
            let vecf: Vec<_> = drafts
                .into_iter()
                .map(|bonsai| repo.get_changeset_parents_by_bonsai(ctx.clone(), &bonsai))
                .collect();

            // join them together and filter already processed
            let parents_fut = stream::futures_unordered(vecf)
                .collect()
                .map(move |parents| {
                    let parents = parents
                        .into_iter()
                        .flat_map(|array| array.into_iter())
                        .filter(|bonsai| !visited.contains(bonsai))
                        .collect::<HashSet<_>>();
                    // update visited hashset
                    visited.extend(parents.iter().cloned());
                    (parents, visited)
                });

            // calculated phases
            let phases_fut = parents_fut.and_then({
                cloned!(ctx, repo, phases_hint);
                move |(parents, visited)| {
                    phases_hint
                        .get_all_with_bookmarks(
                            ctx,
                            repo,
                            parents.into_iter().collect(),
                            maybe_public_heads,
                        )
                        .map(move |phases_mapping| (phases_mapping, visited))
                }
            });

            // return public roots and continue calculation for the remaining drafts
            phases_fut
                .and_then(|(phases_mapping, visited)| {
                    let calculated = phases_mapping.calculated;
                    let maybe_public_heads = phases_mapping.maybe_public_heads;
                    // split by phase
                    let (new_public, new_drafts): (Vec<_>, Vec<_>) = calculated
                        .into_iter()
                        .partition(|(_, phase)| phase == &Phase::Public);
                    // update found public changests
                    public.extend(new_public.into_iter().map(|(cs_id, _)| cs_id));
                    // continue for the new drafts
                    future::ok(future::Loop::Continue((
                        maybe_public_heads,
                        new_drafts.into_iter().map(|(cs_id, _)| cs_id).collect(),
                        public,
                        visited,
                    )))
                })
                .right_future()
        },
    )
    .boxify()
}

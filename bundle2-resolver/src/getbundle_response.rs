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
use futures::{stream, Future, Stream};
use mercurial::{self, RevlogChangeset};
use mercurial_bundles::{part_encode::PartEncodeBuilder, parts};
use mercurial_types::{Changeset, HgBlobNode, HgChangesetId, HgNodeHash, HgPhase, NULL_CSID};
use phases::Phases;
use reachabilityindex::LeastCommonAncestorsHint;
use revset::DifferenceOfUnionsOfAncestorsNodeStream;

use mononoke_types::ChangesetId;

pub fn create_getbundle_response(
    ctx: CoreContext,
    blobrepo: BlobRepo,
    common: Vec<HgChangesetId>,
    heads: Vec<HgChangesetId>,
    lca_hint: Arc<LeastCommonAncestorsHint + Send + Sync>,
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
/// Ignore common because phases might have been changed.
/// TODO (liubovd): if client is pulling non-public changesets, we need to find
/// intermediate public heads and send phases for them as well similar to
/// _getbundlephasespart in mercurial/exchange.py
/// This is to solve the cases when you have a local stack but some public bookmark was been moved to part of it.
fn prepare_phases_stream(
    ctx: CoreContext,
    repo: BlobRepo,
    heads: Vec<HgChangesetId>,
    phases_hint: Arc<Phases>,
) -> impl Stream<Item = (HgNodeHash, HgPhase), Error = Error> {
    repo.get_hg_bonsai_mapping(ctx.clone(), heads)
        .and_then({
            cloned!(ctx, repo);
            move |hg_bonsai_mapping| {
                // convert to bonsai => hg hash map that we will later use
                // skip heads that are not known by the server
                let bonsai_node_mapping: HashMap<ChangesetId, HgNodeHash> = hg_bonsai_mapping
                    .into_iter()
                    .map(|(hg_cs_id, bonsai)| (bonsai, hg_cs_id.into_nodehash()))
                    .collect();
                // calculate phases
                phases_hint
                    .get_all(ctx, repo, bonsai_node_mapping.keys().cloned().collect())
                    .map(move |phases_mapping| (phases_mapping, bonsai_node_mapping))
            }
        })
        .map(move |(phases_mapping, bonsai_node_mapping)| {
            // transform data in a format used by the encoding function
            let calculated: Vec<(HgNodeHash, HgPhase)> = phases_mapping
                .calculated
                .into_iter()
                .map(|(bonsai, phase)| (bonsai_node_mapping[&bonsai], HgPhase::from(phase)))
                .collect();

            stream::iter_ok(calculated)
        })
        .flatten_stream()
}

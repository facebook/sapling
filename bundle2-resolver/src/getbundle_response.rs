// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;
use errors::*;
use failure::err_msg;
use std::collections::HashSet;
use std::iter::FromIterator;
use std::sync::Arc;

use blobrepo::BlobRepo;
use context::CoreContext;
use futures::{stream, Future, Stream};
use mercurial::{self, RevlogChangeset};
use mercurial_bundles::{parts, part_encode::PartEncodeBuilder};
use mercurial_types::{Changeset, HgBlobNode, HgChangesetId, HgPhase, NULL_CSID};
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

    let mut parts = vec![];

    let changesets_buffer_size = 1000; // TODO(stash): make it configurable
    let phases_buffer_size = 1000;

    let phases_part = if let Some(phases_hint) = phases_hint {
        // Phases were requested
        // Calculate phases for the heads.
        // Ignore common because phases might have been changed.
        // TODO (liubovd): if client is pulling non-public changesets, we need to find
        // intermediate public heads and send phases for them as well similar to
        // _getbundlephasespart in mercurial/exchange.py
        // This is to solve the cases when you have a local stack but some public bookmark was been moved to part of it.

        let items = stream::iter_ok(heads.clone().into_iter())
            .map({
                cloned!(ctx, blobrepo, phases_hint);
                move |node| {
                    cloned!(ctx, blobrepo, phases_hint);
                    blobrepo
                        .get_bonsai_from_hg(ctx.clone(), &node)
                        .and_then(move |maybe_bonsai| {
                            maybe_bonsai.ok_or(ErrorKind::BonsaiNotFoundForHgChangeset(node).into())
                        })
                        .and_then(move |bonsai| phases_hint.get(ctx, blobrepo, bonsai))
                        .and_then(move |maybe_phase| {
                            maybe_phase.ok_or(ErrorKind::PhaseUnknownForHgChangeset(node).into())
                        })
                        // transform data in a format used by the encoding function
                        .map(move |phase| (node.into_nodehash(), HgPhase::from(phase)))
                }
            })
            .buffered(phases_buffer_size);
        Some(parts::phases_part(items))
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
    let nodestosend = heads
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
    let nodestosend = nodestosend
        .collect()
        .map(|nodes| stream::iter_ok(nodes.into_iter().rev()))
        .flatten_stream();

    let changelogentries = nodestosend
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

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
use futures::{stream, Future, Stream};
use mercurial::{self, RevlogChangeset};
use mercurial_bundles::{parts, part_encode::PartEncodeBuilder};
use mercurial_types::{Changeset, HgBlobNode, HgChangesetId, NULL_CSID};
use revset::DifferenceOfUnionsOfAncestorsNodeStream;

use mononoke_types::ChangesetId;

pub fn create_getbundle_response(
    blobrepo: BlobRepo,
    common: Vec<HgChangesetId>,
    heads: Vec<HgChangesetId>,
) -> Result<PartEncodeBuilder> {
    if common.is_empty() {
        return Err(err_msg("no 'common' heads specified. Pull will be very inefficient. Please use hg clone instead"));
    }

    let blobrepo = Arc::new(blobrepo.clone());

    let common_heads: HashSet<_> = HashSet::from_iter(common.iter());

    let heads = hg_to_bonsai_stream(
        &blobrepo,
        heads
            .iter()
            .filter(|head| !common_heads.contains(head))
            .cloned()
            .collect(),
    );

    let excludes = hg_to_bonsai_stream(
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
            move |(heads, excludes)| {
                DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
                    &changeset_fetcher,
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

    let buffer_size = 1000; // TODO(stash): make it configurable
    let changelogentries = nodestosend
        .map({
            cloned!(blobrepo);
            move |bonsai| {
                blobrepo
                    .get_hg_from_bonsai_changeset(bonsai)
                    .map(|cs| cs.into_nodehash())
                    .and_then({
                        cloned!(blobrepo);
                        move |node| {
                            blobrepo
                                .get_changeset_by_changesetid(&HgChangesetId::new(node))
                                .map(move |cs| (node, cs))
                        }
                    })
            }
        })
        .buffered(buffer_size)
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

    parts::changegroup_part(changelogentries)
}

fn hg_to_bonsai_stream(
    repo: &Arc<BlobRepo>,
    nodes: Vec<HgChangesetId>,
) -> impl Future<Item = Vec<ChangesetId>, Error = Error> {
    stream::iter_ok(nodes.into_iter())
        .map({
            cloned!(repo);
            move |node| {
                repo.get_bonsai_from_hg(&node)
                    .and_then(move |maybe_bonsai| {
                        maybe_bonsai.ok_or(ErrorKind::BonsaiNotFoundForHgChangeset(node).into())
                    })
            }
        })
        .buffered(100)
        .collect()
}

// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use bonsai_utils::{bonsai_diff, BonsaiDiffResult};
use clap::ArgMatches;
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use failure_ext::{err_msg, format_err, Error};
use futures::prelude::*;
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::{Changeset, HgChangesetId, HgManifestId, MPath};
use revset::RangeNodeStream;
use serde_derive::Serialize;
use slog::Logger;
use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::io;
use std::str::FromStr;

use crate::cmdargs::{HG_CHANGESET_DIFF, HG_CHANGESET_RANGE};

pub fn subcommand_hg_changeset(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    match sub_m.subcommand() {
        (HG_CHANGESET_DIFF, Some(sub_m)) => {
            // TODO(T37478150, luk) This is not a test case, fix it up in future diffs
            let ctx = CoreContext::test_mock();

            let left_cs = sub_m
                .value_of("LEFT_CS")
                .ok_or(format_err!("LEFT_CS argument expected"))
                .and_then(HgChangesetId::from_str);
            let right_cs = sub_m
                .value_of("RIGHT_CS")
                .ok_or(format_err!("RIGHT_CS argument expected"))
                .and_then(HgChangesetId::from_str);

            args::init_cachelib(&matches);
            args::open_repo(&logger, &matches)
                .and_then(move |repo| {
                    (left_cs, right_cs)
                        .into_future()
                        .and_then(move |(left_cs, right_cs)| {
                            hg_changeset_diff(ctx, repo, left_cs, right_cs)
                        })
                })
                .and_then(|diff| {
                    serde_json::to_writer(io::stdout(), &diff)
                        .map(|_| ())
                        .map_err(Error::from)
                })
                .boxify()
        }
        (HG_CHANGESET_RANGE, Some(sub_m)) => {
            let start_cs = sub_m
                .value_of("START_CS")
                .ok_or(format_err!("START_CS argument expected"))
                .and_then(HgChangesetId::from_str);
            let stop_cs = sub_m
                .value_of("STOP_CS")
                .ok_or(format_err!("STOP_CS argument expected"))
                .and_then(HgChangesetId::from_str);

            // TODO(T37478150, luk) This is not a test case, fix it up in future diffs
            let ctx = CoreContext::test_mock();

            args::init_cachelib(&matches);
            args::open_repo(&logger, &matches)
                .and_then(move |repo| {
                    (start_cs, stop_cs)
                        .into_future()
                        .and_then({
                            cloned!(ctx, repo);
                            move |(start_cs, stop_cs)| {
                                (
                                    repo.get_bonsai_from_hg(ctx.clone(), start_cs),
                                    repo.get_bonsai_from_hg(ctx, stop_cs),
                                )
                            }
                        })
                        .and_then(|(start_cs_opt, stop_cs_opt)| {
                            (
                                start_cs_opt.ok_or(err_msg("failed to resolve changeset")),
                                stop_cs_opt.ok_or(err_msg("failed to resovle changeset")),
                            )
                        })
                        .and_then({
                            cloned!(repo);
                            move |(start_cs, stop_cs)| {
                                RangeNodeStream::new(
                                    ctx.clone(),
                                    repo.get_changeset_fetcher(),
                                    start_cs,
                                    stop_cs,
                                )
                                .map(move |cs| repo.get_hg_from_bonsai_changeset(ctx.clone(), cs))
                                .buffered(100)
                                .map(|cs| cs.to_hex().to_string())
                                .collect()
                            }
                        })
                        .and_then(|css| {
                            serde_json::to_writer(io::stdout(), &css)
                                .map(|_| ())
                                .map_err(Error::from)
                        })
                })
                .boxify()
        }
        _ => {
            println!("{}", sub_m.usage());
            ::std::process::exit(1);
        }
    }
}

fn hg_changeset_diff(
    ctx: CoreContext,
    repo: BlobRepo,
    left_id: HgChangesetId,
    right_id: HgChangesetId,
) -> impl Future<Item = ChangesetDiff, Error = Error> {
    (
        repo.get_changeset_by_changesetid(ctx.clone(), left_id),
        repo.get_changeset_by_changesetid(ctx.clone(), right_id),
    )
        .into_future()
        .and_then({
            cloned!(repo, left_id, right_id);
            move |(left, right)| {
                let mut diff = ChangesetDiff {
                    left: left_id,
                    right: right_id,
                    diff: Vec::new(),
                };

                if left.user() != right.user() {
                    diff.diff.push(ChangesetAttrDiff::User(
                        slice_to_str(left.user()),
                        slice_to_str(right.user()),
                    ));
                }

                if left.comments() != right.comments() {
                    diff.diff.push(ChangesetAttrDiff::Comments(
                        slice_to_str(left.comments()),
                        slice_to_str(right.comments()),
                    ))
                }

                if left.files() != right.files() {
                    diff.diff.push(ChangesetAttrDiff::Files(
                        left.files().iter().map(mpath_to_str).collect(),
                        right.files().iter().map(mpath_to_str).collect(),
                    ))
                }

                if left.extra() != right.extra() {
                    diff.diff.push(ChangesetAttrDiff::Extra(
                        left.extra()
                            .iter()
                            .map(|(k, v)| (slice_to_str(k), slice_to_str(v)))
                            .collect(),
                        right
                            .extra()
                            .iter()
                            .map(|(k, v)| (slice_to_str(k), slice_to_str(v)))
                            .collect(),
                    ))
                }

                hg_manifest_diff(ctx, repo, left.manifestid(), right.manifestid()).map(
                    move |mdiff| {
                        diff.diff.extend(mdiff);
                        diff
                    },
                )
            }
        })
}

fn hg_manifest_diff(
    ctx: CoreContext,
    repo: BlobRepo,
    left: HgManifestId,
    right: HgManifestId,
) -> impl Future<Item = Option<ChangesetAttrDiff>, Error = Error> {
    bonsai_diff(
        ctx,
        Box::new(repo.get_root_entry(left)),
        Some(Box::new(repo.get_root_entry(right))),
        None,
    )
    .collect()
    .map(|diffs| {
        let diff = diffs.into_iter().fold(
            ManifestDiff {
                modified: Vec::new(),
                deleted: Vec::new(),
            },
            |mut mdiff, diff| {
                match diff {
                    BonsaiDiffResult::Changed(path, ..)
                    | BonsaiDiffResult::ChangedReusedId(path, ..) => {
                        mdiff.modified.push(mpath_to_str(path))
                    }
                    BonsaiDiffResult::Deleted(path) => mdiff.deleted.push(mpath_to_str(path)),
                };
                mdiff
            },
        );
        if diff.modified.is_empty() && diff.deleted.is_empty() {
            None
        } else {
            Some(ChangesetAttrDiff::Manifest(diff))
        }
    })
}

fn slice_to_str(slice: &[u8]) -> String {
    String::from_utf8_lossy(slice).into_owned()
}

fn mpath_to_str<P: Borrow<MPath>>(mpath: P) -> String {
    let bytes = mpath.borrow().to_vec();
    String::from_utf8_lossy(bytes.as_ref()).into_owned()
}

#[derive(Serialize)]
struct ChangesetDiff {
    left: HgChangesetId,
    right: HgChangesetId,
    diff: Vec<ChangesetAttrDiff>,
}

#[derive(Serialize)]
enum ChangesetAttrDiff {
    #[serde(rename = "user")]
    User(String, String),
    #[serde(rename = "comments")]
    Comments(String, String),
    #[serde(rename = "manifest")]
    Manifest(ManifestDiff),
    #[serde(rename = "files")]
    Files(Vec<String>, Vec<String>),
    #[serde(rename = "extra")]
    Extra(BTreeMap<String, String>, BTreeMap<String, String>),
}

#[derive(Serialize)]
struct ManifestDiff {
    modified: Vec<String>,
    deleted: Vec<String>,
}

/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::error::SubcommandError;
use anyhow::{format_err, Error};
use blame::{fetch_blame, fetch_file_full_content};
use blobrepo::BlobRepo;
use blobstore::{Blobstore, Loadable};
use bytes::Bytes;
use clap::{App, Arg, ArgMatches, SubCommand};
use cloned::cloned;
use cmdlib::{args, helpers};
use context::CoreContext;
use derived_data::BonsaiDerived;
use fbinit::FacebookInit;
use futures::{future, Future, IntoFuture};
use futures_ext::{
    bounded_traversal::{bounded_traversal_dag, Iter},
    BoxFuture, FutureExt,
};
use manifest::ManifestOps;
use mononoke_types::{
    blame::{Blame, BlameRejected},
    ChangesetId, FileUnodeId, MPath,
};
use slog::Logger;
use std::{collections::HashMap, sync::Arc};
use unodes::{RootUnodeManifestId, RootUnodeManifestMapping};

const COMMAND_DERIVE: &'static str = "derive";
const COMMAND_COMPUTE: &'static str = "compute";
const COMMAND_DIFF: &'static str = "diff";

const ARG_CSID: &'static str = "csid";
const ARG_PATH: &'static str = "path";

pub fn subcommand_blame_build(name: &str) -> App {
    let csid_arg = Arg::with_name(ARG_CSID)
        .help("{hg|bonsai} changeset id or bookmark name")
        .index(1)
        .required(true);
    let path_arg = Arg::with_name(ARG_PATH)
        .help("path")
        .index(2)
        .required(true);

    SubCommand::with_name(name)
        .about("fetch/derive blame for specified changeset and path")
        .subcommand(
            SubCommand::with_name(COMMAND_DERIVE)
                .arg(csid_arg.clone())
                .arg(path_arg.clone()),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_DIFF)
                .arg(csid_arg.clone())
                .arg(path_arg.clone()),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_COMPUTE)
                .arg(csid_arg.clone())
                .arg(path_arg.clone()),
        )
}

pub fn subcommand_blame(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_matches: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    args::init_cachelib(fb, &matches);

    let repo = args::open_repo(fb, &logger, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    match sub_matches.subcommand() {
        (COMMAND_DERIVE, Some(matches)) => {
            with_changeset_and_path(ctx, repo, matches, subcommand_show_blame)
        }
        (COMMAND_DIFF, Some(matches)) => {
            with_changeset_and_path(ctx, repo, matches, subcommand_show_diffs)
        }
        (COMMAND_COMPUTE, Some(matches)) => {
            with_changeset_and_path(ctx, repo, matches, subcommand_compute_blame)
        }
        _ => future::err(SubcommandError::InvalidArgs).boxify(),
    }
}

fn with_changeset_and_path<F, FOut>(
    ctx: CoreContext,
    repo: impl Future<Item = BlobRepo, Error = Error> + Send + 'static,
    matches: &ArgMatches<'_>,
    fun: F,
) -> BoxFuture<(), SubcommandError>
where
    F: FnOnce(CoreContext, BlobRepo, ChangesetId, MPath) -> FOut + Send + 'static,
    FOut: Future<Item = (), Error = Error> + Send + 'static,
{
    let hash_or_bookmark = String::from(matches.value_of(ARG_CSID).unwrap());
    let path = MPath::new(matches.value_of(ARG_PATH).unwrap());
    (repo, path)
        .into_future()
        .and_then({
            move |(repo, path)| {
                helpers::csid_resolve(ctx.clone(), repo.clone(), hash_or_bookmark)
                    .and_then(move |csid| fun(ctx, repo, csid, path))
            }
        })
        .from_err()
        .boxify()
}

fn subcommand_show_blame(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    path: MPath,
) -> impl Future<Item = (), Error = Error> {
    fetch_blame(ctx.clone(), repo.clone(), csid, path)
        .from_err()
        .and_then(move |(content, blame)| {
            blame_hg_annotate(ctx, repo, content, blame).map(|annotate| println!("{}", annotate))
        })
}

fn find_leaf(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    path: MPath,
) -> impl Future<Item = FileUnodeId, Error = Error> {
    let blobstore = repo.get_blobstore();
    let unodes_mapping = Arc::new(RootUnodeManifestMapping::new(blobstore.clone()));
    RootUnodeManifestId::derive(ctx.clone(), repo, unodes_mapping, csid)
        .and_then({
            cloned!(blobstore, path);
            move |mf_root| {
                mf_root
                    .manifest_unode_id()
                    .clone()
                    .find_entry(ctx, blobstore, Some(path))
            }
        })
        .and_then({
            cloned!(path);
            move |entry_opt| {
                let entry = entry_opt.ok_or_else(|| format_err!("No such path: {}", path))?;
                match entry.into_leaf() {
                    None => Err(format_err!(
                        "Blame is not available for directories: {}",
                        path
                    )),
                    Some(file_unode_id) => Ok(file_unode_id),
                }
            }
        })
}

fn subcommand_show_diffs(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    path: MPath,
) -> impl Future<Item = (), Error = Error> {
    let blobstore = repo.get_blobstore();
    find_leaf(ctx.clone(), repo, csid, path)
        .and_then({
            cloned!(ctx, blobstore);
            move |file_unode_id| {
                file_unode_id
                    .load(ctx, &blobstore)
                    .from_err()
                    .map(move |file_unode| (file_unode_id, file_unode))
            }
        })
        .and_then(move |(file_unode_id, file_unode)| {
            let diffs = file_unode
                .parents()
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .map({
                    cloned!(ctx, blobstore);
                    move |parent| diff(ctx.clone(), blobstore.boxed(), file_unode_id, parent)
                });
            future::join_all(diffs).map(|diffs| {
                for diff in diffs {
                    println!("{}", diff);
                }
            })
        })
}

fn diff(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    new: FileUnodeId,
    old: FileUnodeId,
) -> impl Future<Item = String, Error = Error> {
    (
        fetch_file_full_content(ctx.clone(), blobstore.clone(), new)
            .and_then(|result| result.map_err(Error::from)),
        fetch_file_full_content(ctx, blobstore, old).and_then(|result| result.map_err(Error::from)),
    )
        .into_future()
        .map(|(new, old)| {
            let new = xdiff::DiffFile {
                path: "new",
                contents: new,
                file_type: xdiff::FileType::Regular,
            };
            let old = xdiff::DiffFile {
                path: "old",
                contents: old,
                file_type: xdiff::FileType::Regular,
            };
            let diff = xdiff::diff_unified(
                Some(old),
                Some(new),
                xdiff::DiffOpts {
                    context: 3,
                    copy_info: xdiff::CopyInfo::None,
                },
            );
            String::from_utf8_lossy(&diff).into_owned()
        })
}

/// Recalculate balme by going through whole history of a file
fn subcommand_compute_blame(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    path: MPath,
) -> impl Future<Item = (), Error = Error> {
    let blobstore = repo.get_blobstore().boxed();
    find_leaf(ctx.clone(), repo.clone(), csid, path.clone())
        .and_then({
            cloned!(ctx, repo);
            move |file_unode_id| {
                bounded_traversal_dag(
                    256,
                    (file_unode_id, path),
                    {
                        // unfold operator traverses all parents of a given unode, accounting for
                        // renames and treating them as another parent.
                        cloned!(ctx, repo, blobstore);
                        move |(file_unode_id, path)| {
                            cloned!(ctx, repo, blobstore);
                            file_unode_id
                                .load(ctx.clone(), &blobstore)
                                .from_err()
                                .and_then(move |file_unode| {
                                    let csid = *file_unode.linknode();
                                    csid
                                        .load(ctx.clone(), &blobstore)
                                        .from_err()
                                        .and_then({
                                            cloned!(ctx, repo, path);
                                            move |bonsai| {
                                                let copy_from = bonsai.file_changes_map()
                                                    .get(&path)
                                                    .and_then(|file_change| file_change.as_ref())
                                                    .and_then(|file_change| file_change.copy_from().clone());
                                                match copy_from {
                                                    None => future::ok(None).left_future(),
                                                    Some((r_path, r_csid)) => {
                                                        find_leaf(ctx, repo, *r_csid, r_path.clone())
                                                            .map({
                                                                cloned!(r_path);
                                                                move |r_unode_id| Some((r_unode_id, r_path))
                                                            })
                                                            .right_future()
                                                    }
                                                }
                                            }
                                        })
                                        .map(move |copy_parent| {
                                            let parents: Vec<_> = file_unode
                                                .parents()
                                                .iter()
                                                .map(|unode_id| (*unode_id, path.clone()))
                                                .chain(copy_parent)
                                                .collect();
                                            (
                                                (csid, path, file_unode_id),
                                                parents,
                                            )
                                        })
                                })
                        }
                    },
                    {
                        move |(csid, path, file_unode_id), parents: Iter<Result<(Bytes, Blame), BlameRejected>>| {
                            cloned!(path);
                            fetch_file_full_content(ctx.clone(), blobstore.clone(), file_unode_id)
                                .and_then(move |content| match content {
                                    Err(rejected) => Ok(Err(rejected)),
                                    Ok(content) => {
                                        let parents = parents
                                            .into_iter()
                                            .filter_map(|result| result.ok())
                                            .collect();
                                        Blame::from_parents(
                                            csid,
                                            content.clone(),
                                            path.clone(),
                                            parents,
                                        )
                                        .map(move |blame| Ok((content, blame)))
                                    }
                                })
                        }
                    },
                )
            }
        })
        .and_then(|result| {
            Ok(result.ok_or_else(|| Error::msg("cycle found"))??)
        })
        .and_then(move |(content, blame)| {
            blame_hg_annotate(ctx, repo, content, blame).map(|annotate| println!("{}", annotate))
        })
}

/// Format blame the same way `hg blame` does
fn blame_hg_annotate<C: AsRef<[u8]> + 'static>(
    ctx: CoreContext,
    repo: BlobRepo,
    content: C,
    blame: Blame,
) -> impl Future<Item = String, Error = Error> {
    if content.as_ref().is_empty() {
        return future::ok(String::new()).left_future();
    }

    let csids: Vec<_> = blame.ranges().iter().map(|range| range.csid).collect();
    repo.get_hg_bonsai_mapping(ctx, csids)
        .and_then(move |mapping| {
            let mapping: HashMap<_, _> = mapping.into_iter().map(|(k, v)| (v, k)).collect();

            let content = String::from_utf8_lossy(content.as_ref());
            let mut result = String::new();
            let mut ranges = blame.ranges().iter();
            let mut range = ranges
                .next()
                .ok_or_else(|| Error::msg("empty blame for non empty content"))?;
            for (index, line) in content.lines().enumerate() {
                if index as u32 >= range.offset + range.length {
                    range = ranges
                        .next()
                        .ok_or_else(|| Error::msg("not enough ranges in a blame"))?;
                }
                let hg_csid = mapping
                    .get(&range.csid)
                    .ok_or_else(|| format_err!("unresolved bonsai csid: {}", range.csid))?;
                result.push_str(&hg_csid.to_string()[..12]);
                result.push_str(": ");
                result.push_str(line);
                result.push_str("\n");
            }
            result.pop();
            Ok(result)
        })
        .right_future()
}

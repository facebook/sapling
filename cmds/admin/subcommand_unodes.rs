// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::error::SubcommandError;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use clap::{App, Arg, ArgMatches, SubCommand};
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use derive_unode_manifest::derived_data_unodes::{RootUnodeManifestId, RootUnodeManifestMapping};
use derived_data::{BonsaiDerived, RegenerateMapping};
use failure::{err_msg, Error};
use fbinit::FacebookInit;
use futures::{future, Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use manifest::{Entry, ManifestOps, PathOrPrefix};
use mercurial_types::Changeset;
use mononoke_types::{ChangesetId, MPath};
use revset::AncestorsNodeStream;
use slog::Logger;
use std::{collections::BTreeSet, sync::Arc};

const COMMAND_TREE: &'static str = "tree";
const COMMAND_VERIFY: &'static str = "verify";
const COMMAND_REGENERATE: &'static str = "regenerate";
const ARG_CSID: &'static str = "csid";
const ARG_PATH: &'static str = "path";
const ARG_LIMIT: &'static str = "limit";
const ARG_TRACE: &'static str = "trace";

fn path_resolve(path: &str) -> Result<Option<MPath>, Error> {
    match path {
        "/" => Ok(None),
        _ => Ok(Some(MPath::new(path)?)),
    }
}

pub fn subcommand_unodes_build(name: &str) -> App {
    let csid_arg = Arg::with_name(ARG_CSID)
        .help("{hg|boinsai} changset id or bookmark name")
        .index(1)
        .required(true);

    let path_arg = Arg::with_name(ARG_PATH)
        .help("path")
        .index(2)
        .default_value("/");

    SubCommand::with_name(name)
        .about("inspect and interact with unodes")
        .arg(
            Arg::with_name(ARG_TRACE)
                .help("upload trace to manifold")
                .long("trace"),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_TREE)
                .help("recursively list all unode entries starting with prefix")
                .arg(csid_arg.clone())
                .arg(path_arg.clone()),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_VERIFY)
                .help("verify unode tree agains hg-manifest")
                .arg(csid_arg.clone())
                .arg(
                    Arg::with_name(ARG_LIMIT)
                        .help("number of commits to be verified")
                        .takes_value(true)
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_REGENERATE)
                .help("regenerate unode")
                .arg(csid_arg.clone()),
        )
}

pub fn subcommand_unodes(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_matches: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    let tracing_enable = sub_matches.is_present(ARG_TRACE);
    if tracing_enable {
        tracing::enable();
    }

    args::init_cachelib(fb, &matches);

    let repo = args::open_repo(fb, &logger, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let run = match sub_matches.subcommand() {
        (COMMAND_TREE, Some(matches)) => {
            let hash_or_bookmark = String::from(matches.value_of(ARG_CSID).unwrap());
            let path = path_resolve(matches.value_of(ARG_PATH).unwrap());
            cloned!(ctx);
            (repo, path)
                .into_future()
                .and_then(move |(repo, path)| {
                    args::csid_resolve(ctx.clone(), repo.clone(), hash_or_bookmark)
                        .and_then(move |csid| subcommand_tree(ctx, repo, csid, path))
                })
                .from_err()
                .boxify()
        }
        (COMMAND_VERIFY, Some(matches)) => {
            let hash_or_bookmark = String::from(matches.value_of(ARG_CSID).unwrap());
            let limit = matches
                .value_of(ARG_LIMIT)
                .unwrap()
                .parse::<u64>()
                .expect("limit must be an integer");
            cloned!(ctx);
            repo.into_future()
                .and_then(move |repo| {
                    args::csid_resolve(ctx.clone(), repo.clone(), hash_or_bookmark)
                        .and_then(move |csid| subcommand_verify(ctx, repo, csid, limit))
                })
                .from_err()
                .boxify()
        }
        (COMMAND_REGENERATE, Some(matches)) => {
            let hash_or_bookmark = String::from(matches.value_of(ARG_CSID).unwrap());
            repo.into_future()
                .and_then({
                    cloned!(ctx);
                    move |repo| {
                        args::csid_resolve(ctx.clone(), repo.clone(), hash_or_bookmark)
                            .map(move |csid| (ctx, repo, csid))
                    }
                })
                .and_then(move |(ctx, repo, csid)| {
                    let mapping =
                        RegenerateMapping::new(RootUnodeManifestMapping::new(repo.get_blobstore()));
                    mapping.regenerate(Some(csid));
                    RootUnodeManifestId::derive(ctx.clone(), repo.clone(), mapping, csid)
                        .map(move |root| println!("{:?} -> {:?}", csid, root))
                })
                .from_err()
                .boxify()
        }
        _ => future::err(SubcommandError::InvalidArgs).boxify(),
    };

    if tracing_enable {
        run.then(move |result| ctx.trace_upload().then(move |_| result))
            .boxify()
    } else {
        run
    }
}

fn subcommand_tree(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    path: Option<MPath>,
) -> impl Future<Item = (), Error = Error> {
    let mapping = Arc::new(RootUnodeManifestMapping::new(repo.get_blobstore()));
    RootUnodeManifestId::derive(ctx.clone(), repo.clone(), mapping, csid).and_then(move |root| {
        println!("ROOT: {:?}", root);
        println!("PATH: {:?}", path);
        root.manifest_unode_id()
            .find_entries(ctx, repo.get_blobstore(), vec![PathOrPrefix::Prefix(path)])
            .for_each(|(path, entry)| {
                match entry {
                    Entry::Tree(tree_id) => {
                        println!("{}/ {:?}", MPath::display_opt(path.as_ref()), tree_id);
                    }
                    Entry::Leaf(leaf_id) => {
                        println!("{} {:?}", MPath::display_opt(path.as_ref()), leaf_id);
                    }
                }
                Ok(())
            })
    })
}

fn subcommand_verify(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    limit: u64,
) -> impl Future<Item = (), Error = Error> {
    AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), csid)
        .take(limit)
        .for_each(move |csid| single_verify(ctx.clone(), repo.clone(), csid))
}

fn single_verify(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
) -> impl Future<Item = (), Error = Error> {
    let hg_paths = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), csid)
        .and_then({
            cloned!(ctx, repo);
            move |hg_csid| {
                println!("CHANGESET: hg_csid:{:?} csid:{:?}", hg_csid, csid);
                hg_csid.load(ctx.clone(), &repo.get_blobstore()).from_err()
            }
        })
        .and_then({
            cloned!(ctx, repo);
            move |hg_changeset| {
                hg_changeset
                    .manifestid()
                    .find_entries(ctx, repo.get_blobstore(), vec![PathOrPrefix::Prefix(None)])
                    .filter_map(|(path, _)| path)
                    .collect_to::<BTreeSet<_>>()
            }
        });

    let mapping = Arc::new(RootUnodeManifestMapping::new(repo.get_blobstore()));
    let unode_paths = RootUnodeManifestId::derive(ctx.clone(), repo.clone(), mapping, csid)
        .and_then(move |tree_id| {
            tree_id
                .manifest_unode_id()
                .find_entries(ctx, repo.get_blobstore(), vec![PathOrPrefix::Prefix(None)])
                .filter_map(|(path, _)| path)
                .collect_to::<BTreeSet<_>>()
        });

    (hg_paths, unode_paths)
        .into_future()
        .and_then(|(hg_paths, unode_paths)| {
            if hg_paths == unode_paths {
                Ok(())
            } else {
                println!("DIFFERENT: +hg -unode");
                for path in hg_paths.difference(&unode_paths) {
                    println!("+ {}", path);
                }
                for path in unode_paths.difference(&hg_paths) {
                    println!("- {}", path);
                }
                Err(err_msg("failed"))
            }
        })
}

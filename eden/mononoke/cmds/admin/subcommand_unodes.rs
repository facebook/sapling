/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::error::SubcommandError;

use anyhow::bail;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use context::CoreContext;
use derived_data::BonsaiDerived;
use fbinit::FacebookInit;
use futures::compat::Stream01CompatExt;
use futures::StreamExt;
use futures::TryStreamExt;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::PathOrPrefix;
use mercurial_derived_data::DeriveHgChangeset;

use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use revset::AncestorsNodeStream;
use slog::info;
use slog::Logger;
use std::collections::BTreeSet;
use unodes::RootUnodeManifestId;

pub const UNODES: &str = "unodes";
const COMMAND_TREE: &str = "tree";
const COMMAND_VERIFY: &str = "verify";
const ARG_CSID: &str = "csid";
const ARG_PATH: &str = "path";
const ARG_LIMIT: &str = "limit";

fn path_resolve(path: &str) -> Result<Option<MPath>, Error> {
    match path {
        "/" => Ok(None),
        _ => Ok(Some(MPath::new(path)?)),
    }
}

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    let csid_arg = Arg::with_name(ARG_CSID)
        .help("{hg|bonsai} changeset id or bookmark name")
        .index(1)
        .required(true);

    let path_arg = Arg::with_name(ARG_PATH)
        .help("path")
        .index(2)
        .default_value("/");

    SubCommand::with_name(UNODES)
        .about("inspect and interact with unodes")
        .subcommand(
            SubCommand::with_name(COMMAND_TREE)
                .about("recursively list all unode entries starting with prefix")
                .arg(csid_arg.clone())
                .arg(path_arg.clone()),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_VERIFY)
                .about("verify unode tree agains hg-manifest")
                .arg(csid_arg.clone())
                .arg(
                    Arg::with_name(ARG_LIMIT)
                        .help("number of commits to be verified")
                        .takes_value(true)
                        .required(true),
                ),
        )
}

pub async fn subcommand_unodes<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_matches: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let repo: BlobRepo = args::open_repo(fb, &logger, matches).await?;
    let ctx = CoreContext::new_with_logger(fb, logger);

    let res = match sub_matches.subcommand() {
        (COMMAND_TREE, Some(matches)) => {
            let hash_or_bookmark = String::from(matches.value_of(ARG_CSID).unwrap());
            let path = path_resolve(matches.value_of(ARG_PATH).unwrap())?;
            let csid = helpers::csid_resolve(&ctx, repo.clone(), hash_or_bookmark).await?;
            subcommand_tree(&ctx, repo, csid, path)
                .await
                .map_err(SubcommandError::Error)
        }
        (COMMAND_VERIFY, Some(matches)) => {
            let hash_or_bookmark = String::from(matches.value_of(ARG_CSID).unwrap());
            let limit = matches
                .value_of(ARG_LIMIT)
                .unwrap()
                .parse::<u64>()
                .expect("limit must be an integer");
            let csid = helpers::csid_resolve(&ctx, repo.clone(), hash_or_bookmark).await?;
            subcommand_verify(&ctx, repo, csid, limit)
                .await
                .map_err(SubcommandError::Error)
        }
        _ => Err(SubcommandError::InvalidArgs),
    };

    res
}

async fn subcommand_tree(
    ctx: &CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    path: Option<MPath>,
) -> Result<(), Error> {
    let root = RootUnodeManifestId::derive(ctx, &repo, csid).await?;
    info!(ctx.logger(), "ROOT: {:?}", root);
    info!(ctx.logger(), "PATH: {:?}", path);
    root.manifest_unode_id()
        .find_entries(
            ctx.clone(),
            repo.get_blobstore(),
            vec![PathOrPrefix::Prefix(path)],
        )
        .try_for_each(|(path, entry)| async move {
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
        .await
}

async fn subcommand_verify(
    ctx: &CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    limit: u64,
) -> Result<(), Error> {
    AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), csid)
        .compat()
        .take(limit as usize)
        .try_for_each(|csid| single_verify(ctx, &repo, csid))
        .await
}

async fn single_verify(ctx: &CoreContext, repo: &BlobRepo, csid: ChangesetId) -> Result<(), Error> {
    let hg_paths = async move {
        let hg_csid = repo.derive_hg_changeset(ctx, csid).await?;
        println!("CHANGESET: hg_csid:{:?} csid:{:?}", hg_csid, csid);
        let hg_changeset = hg_csid.load(ctx, repo.blobstore()).await?;
        let paths = hg_changeset
            .manifestid()
            .find_entries(
                ctx.clone(),
                repo.get_blobstore(),
                vec![PathOrPrefix::Prefix(None)],
            )
            .try_filter_map(|(path, _)| async move { Ok(path) })
            .try_collect::<BTreeSet<_>>()
            .await?;
        Ok::<_, Error>(paths)
    };

    let unode_paths = async move {
        let tree_id = RootUnodeManifestId::derive(ctx, repo, csid).await?;
        let paths = tree_id
            .manifest_unode_id()
            .find_entries(
                ctx.clone(),
                repo.get_blobstore(),
                vec![PathOrPrefix::Prefix(None)],
            )
            .try_filter_map(|(path, _)| async { Ok(path) })
            .try_collect::<BTreeSet<_>>()
            .await?;
        Ok(paths)
    };

    let (hg_paths, unode_paths) = futures::try_join!(hg_paths, unode_paths)?;
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
        bail!("failed")
    }
}

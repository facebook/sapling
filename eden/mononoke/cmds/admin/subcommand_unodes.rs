/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::error::SubcommandError;

use anyhow::{bail, Error};
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use clap::{App, Arg, ArgMatches, SubCommand};
use cloned::cloned;
use cmdlib::{
    args::{self, MononokeMatches},
    helpers,
};
use context::CoreContext;
use derived_data::BonsaiDerived;
use fbinit::FacebookInit;
use futures::{compat::Future01CompatExt, FutureExt, TryFutureExt, TryStreamExt};
use futures_ext::StreamExt;
use futures_old::{Future, IntoFuture, Stream};
use manifest::{Entry, ManifestOps, PathOrPrefix};

use mononoke_types::{ChangesetId, MPath};
use revset::AncestorsNodeStream;
use slog::{info, Logger};
use std::collections::BTreeSet;
use unodes::RootUnodeManifestId;

pub const UNODES: &str = "unodes";
const COMMAND_TREE: &str = "tree";
const COMMAND_VERIFY: &str = "verify";
const ARG_CSID: &str = "csid";
const ARG_PATH: &str = "path";
const ARG_LIMIT: &str = "limit";
const ARG_TRACE: &str = "trace";

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
        .arg(
            Arg::with_name(ARG_TRACE)
                .help("upload trace to manifold")
                .long("trace"),
        )
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
    let tracing_enable = sub_matches.is_present(ARG_TRACE);
    if tracing_enable {
        tracing::enable();
    }

    args::init_cachelib(fb, &matches);

    let repo = args::open_repo(fb, &logger, &matches).await?;
    let ctx = CoreContext::new_with_logger(fb, logger);

    let res = match sub_matches.subcommand() {
        (COMMAND_TREE, Some(matches)) => {
            let hash_or_bookmark = String::from(matches.value_of(ARG_CSID).unwrap());
            let path = path_resolve(matches.value_of(ARG_PATH).unwrap())?;
            let csid = helpers::csid_resolve(ctx.clone(), repo.clone(), hash_or_bookmark)
                .compat()
                .await?;
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
            let csid = helpers::csid_resolve(ctx.clone(), repo.clone(), hash_or_bookmark)
                .compat()
                .await?;
            subcommand_verify(&ctx, repo, csid, limit)
                .await
                .map_err(SubcommandError::Error)
        }
        _ => Err(SubcommandError::InvalidArgs),
    };

    if tracing_enable {
        ctx.trace_upload().compat().await?;
    }
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
        .compat()
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
        .compat()
        .await
}

async fn subcommand_verify(
    ctx: &CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    limit: u64,
) -> Result<(), Error> {
    AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), csid)
        .take(limit)
        .for_each(move |csid| single_verify(ctx.clone(), repo.clone(), csid))
        .compat()
        .await
}

fn single_verify(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
) -> impl Future<Item = (), Error = Error> {
    let hg_paths = {
        cloned!(ctx, repo);
        async move { repo.get_hg_from_bonsai_changeset(ctx, csid).await }
    }
    .boxed()
    .compat()
    .and_then({
        cloned!(ctx, repo);
        move |hg_csid| {
            println!("CHANGESET: hg_csid:{:?} csid:{:?}", hg_csid, csid);
            cloned!(ctx);
            let blobstore = repo.get_blobstore();
            async move { hg_csid.load(&ctx, &blobstore).await }
                .boxed()
                .compat()
                .from_err()
        }
    })
    .and_then({
        cloned!(ctx, repo);
        move |hg_changeset| {
            hg_changeset
                .manifestid()
                .find_entries(ctx, repo.get_blobstore(), vec![PathOrPrefix::Prefix(None)])
                .compat()
                .filter_map(|(path, _)| path)
                .collect_to::<BTreeSet<_>>()
        }
    });

    let unode_paths = {
        cloned!(ctx, repo);
        async move { Ok(RootUnodeManifestId::derive(&ctx, &repo, csid).await?) }
            .boxed()
            .compat()
    }
    .and_then(move |tree_id| {
        tree_id
            .manifest_unode_id()
            .find_entries(ctx, repo.get_blobstore(), vec![PathOrPrefix::Prefix(None)])
            .compat()
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
                bail!("failed")
            }
        })
}

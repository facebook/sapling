/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::derived_data::derive_or_fetch;
use crate::error::SubcommandError;

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
use fbinit::FacebookInit;
use futures::stream::StreamExt;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::ManifestOrderedOps;
use manifest::PathOrPrefix;

use mononoke_types::skeleton_manifest::SkeletonManifestEntry;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use skeleton_manifest::RootSkeletonManifestId;
use slog::info;
use slog::Logger;

pub const SKELETON_MANIFESTS: &str = "skeleton-manifests";
const COMMAND_TREE: &str = "tree";
const COMMAND_LIST: &str = "list";
const ARG_CSID: &str = "csid";
const ARG_PATH: &str = "path";
const ARG_IF_DERIVED: &str = "if-derived";
const ARG_ORDERED: &str = "ordered";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(SKELETON_MANIFESTS)
        .about("inspect skeleton manifests")
        .subcommand(
            SubCommand::with_name(COMMAND_TREE)
                .about("recursively list all skeleton manifest entries starting with prefix")
                .arg(
                    Arg::with_name(ARG_CSID)
                        .help("{hg|bonsai} changeset id or bookmark name")
                        .required(true),
                )
                .arg(
                    Arg::with_name(ARG_IF_DERIVED)
                        .help("only list the manifests if they are already derived")
                        .long(ARG_IF_DERIVED),
                )
                .arg(
                    Arg::with_name(ARG_ORDERED)
                        .help("list the manifest in order")
                        .long(ARG_ORDERED),
                )
                .arg(Arg::with_name(ARG_PATH).help("path")),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_LIST)
                .about("list all skeleton manifest entries in a directory")
                .arg(
                    Arg::with_name(ARG_CSID)
                        .help("{hg|bonsai} changeset id or bookmark name")
                        .required(true),
                )
                .arg(
                    Arg::with_name(ARG_IF_DERIVED)
                        .help("only list the manifests if they are already derived")
                        .long(ARG_IF_DERIVED),
                )
                .arg(Arg::with_name(ARG_PATH).help("path")),
        )
}

pub async fn subcommand_skeleton_manifests<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_matches: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let repo: BlobRepo = args::open_repo(fb, &logger, matches).await?;
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    match sub_matches.subcommand() {
        (COMMAND_TREE, Some(matches)) => {
            let hash_or_bookmark = String::from(matches.value_of(ARG_CSID).unwrap());
            let path = matches.value_of(ARG_PATH).map(MPath::new).transpose()?;

            let csid = helpers::csid_resolve(&ctx, repo.clone(), hash_or_bookmark).await?;
            let fetch_derived = matches.is_present(ARG_IF_DERIVED);
            let ordered = matches.is_present(ARG_ORDERED);
            subcommand_tree(&ctx, &repo, csid, path, fetch_derived, ordered).await?;
            Ok(())
        }
        (COMMAND_LIST, Some(matches)) => {
            let hash_or_bookmark = String::from(matches.value_of(ARG_CSID).unwrap());
            let path = matches.value_of(ARG_PATH).map(MPath::new).transpose()?;

            let csid = helpers::csid_resolve(&ctx, repo.clone(), hash_or_bookmark).await?;
            let fetch_derived = matches.is_present(ARG_IF_DERIVED);
            subcommand_list(&ctx, &repo, csid, path, fetch_derived).await?;
            Ok(())
        }
        _ => Err(SubcommandError::InvalidArgs),
    }
}

async fn subcommand_list(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csid: ChangesetId,
    path: Option<MPath>,
    fetch_derived: bool,
) -> Result<(), Error> {
    let root = derive_or_fetch::<RootSkeletonManifestId>(ctx, repo, csid, fetch_derived).await?;

    info!(ctx.logger(), "ROOT: {:?}", root);
    info!(ctx.logger(), "PATH: {:?}", path);

    match root
        .skeleton_manifest_id()
        .find_entry(ctx.clone(), repo.get_blobstore(), path.clone())
        .await?
    {
        Some(Entry::Tree(skeleton_id)) => {
            for (elem, entry) in skeleton_id.load(ctx, repo.blobstore()).await?.list() {
                match entry {
                    SkeletonManifestEntry::Directory(..) => {
                        println!("{}/", MPath::join_opt_element(path.as_ref(), elem));
                    }
                    SkeletonManifestEntry::File => {
                        println!("{}", MPath::join_opt_element(path.as_ref(), elem));
                    }
                }
            }
        }
        Some(Entry::Leaf(())) => println!("{}", MPath::display_opt(path.as_ref())),
        None => {}
    }

    Ok(())
}

async fn subcommand_tree(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csid: ChangesetId,
    path: Option<MPath>,
    fetch_derived: bool,
    ordered: bool,
) -> Result<(), Error> {
    let root = derive_or_fetch::<RootSkeletonManifestId>(ctx, repo, csid, fetch_derived).await?;

    info!(ctx.logger(), "ROOT: {:?}", root);
    info!(ctx.logger(), "PATH: {:?}", path);

    let mut stream = if ordered {
        root.skeleton_manifest_id()
            .find_entries_ordered(
                ctx.clone(),
                repo.get_blobstore(),
                vec![PathOrPrefix::Prefix(path)],
                None,
            )
            .left_stream()
    } else {
        root.skeleton_manifest_id()
            .find_entries(
                ctx.clone(),
                repo.get_blobstore(),
                vec![PathOrPrefix::Prefix(path)],
            )
            .right_stream()
    };

    while let Some((path, entry)) = stream.next().await.transpose()? {
        match entry {
            Entry::Tree(..) => {}
            Entry::Leaf(()) => {
                println!("{}", MPath::display_opt(path.as_ref()),);
            }
        };
    }

    Ok(())
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::error::SubcommandError;

use anyhow::Error;
use blobrepo::BlobRepo;
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
use futures::stream::StreamExt;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::PathOrPrefix;

use fsnodes::RootFsnodeId;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use slog::info;
use slog::Logger;

pub const FSNODES: &str = "fsnodes";
const COMMAND_TREE: &str = "tree";
const ARG_CSID: &str = "csid";
const ARG_PATH: &str = "path";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(FSNODES)
        .about("inspect fsnodes")
        .subcommand(
            SubCommand::with_name(COMMAND_TREE)
                .about("recursively list all fsnode entries starting with prefix")
                .arg(
                    Arg::with_name(ARG_CSID)
                        .help("{hg|bonsai} changeset id or bookmark name")
                        .required(true),
                )
                .arg(Arg::with_name(ARG_PATH).help("path")),
        )
}

pub async fn subcommand_fsnodes<'a>(
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
            subcommand_tree(&ctx, &repo, csid, path).await?;
            Ok(())
        }
        _ => Err(SubcommandError::InvalidArgs),
    }
}

async fn subcommand_tree(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csid: ChangesetId,
    path: Option<MPath>,
) -> Result<(), Error> {
    let root = RootFsnodeId::derive(ctx, repo, csid).await?;

    info!(ctx.logger(), "ROOT: {:?}", root);
    info!(ctx.logger(), "PATH: {:?}", path);

    let mut stream = root.fsnode_id().find_entries(
        ctx.clone(),
        repo.get_blobstore(),
        vec![PathOrPrefix::Prefix(path)],
    );

    while let Some((path, entry)) = stream.next().await.transpose()? {
        match entry {
            Entry::Tree(..) => {}
            Entry::Leaf(file) => {
                println!(
                    "{}\t{}\t{}\t{}",
                    MPath::display_opt(path.as_ref()),
                    file.content_id(),
                    file.file_type(),
                    file.size(),
                );
            }
        };
    }

    Ok(())
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use fbinit::FacebookInit;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Error;
use blobrepo::BlobRepo;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use context::CoreContext;
use mercurial_derived_data::DeriveHgChangeset;
use mercurial_types::HgChangesetId;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use slog::Logger;

use crate::error::SubcommandError;

pub const HASH_CONVERT: &str = "convert";
const ARG_FROM: &str = "from";
const ARG_TO: &str = "to";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(HASH_CONVERT)
        .about("convert between bonsai and hg changeset hashes")
        .arg(
            Arg::with_name(ARG_FROM)
                .long(ARG_FROM)
                .short("f")
                .required(true)
                .takes_value(true)
                .possible_values(&["hg", "bonsai", "git"])
                .help("Source hash type"),
        )
        .arg(
            Arg::with_name(ARG_TO)
                .long(ARG_TO)
                .short("t")
                .required(true)
                .takes_value(true)
                .possible_values(&["hg", "bonsai", "git"])
                .help("Target hash type"),
        )
        .args_from_usage("<HASH>  'source hash'")
}

pub async fn subcommand_hash_convert<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let source_hash = sub_m.value_of("HASH").unwrap().to_string();
    let source = sub_m.value_of("from").unwrap().to_string();
    let target = sub_m.value_of("to").unwrap();
    // Check that source and target are different types.
    if source == target {
        return Err(anyhow!("source and target should be different").into());
    }
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo: BlobRepo = args::open_repo(fb, &logger, matches).await?;

    let cs_id = convert_to_bonsai(&ctx, &repo, &source, &source_hash).await?;
    println!("{}", convert_from_bonsai(&ctx, &repo, cs_id, target).await?);

    Ok(())
}

async fn convert_to_bonsai(
    ctx: &CoreContext,
    repo: &BlobRepo,
    from: &str,
    hash: &str,
) -> Result<ChangesetId, Error> {
    if from == "hg" {
        let maybebonsai = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(
                ctx,
                HgChangesetId::from_str(hash).expect("source hash is not valid hg changeset id"),
            )
            .await?;

        maybebonsai.ok_or_else(|| anyhow!("bonsai not found for {}", hash))
    } else if from == "git" {
        let maybebonsai = repo
            .bonsai_git_mapping()
            .get_bonsai_from_git_sha1(ctx, GitSha1::from_str(hash)?)
            .await?;

        maybebonsai.ok_or_else(|| anyhow!("git not found for {}", hash))
    } else if from == "bonsai" {
        ChangesetId::from_str(hash)
    } else {
        return Err(anyhow!("unknown source {}", from));
    }
}

async fn convert_from_bonsai(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
    to: &str,
) -> Result<String, Error> {
    if to == "hg" {
        let hg = repo.derive_hg_changeset(ctx, cs_id).await?;
        Ok(format!("{}", hg))
    } else if to == "git" {
        let maybegit = repo
            .bonsai_git_mapping()
            .get_git_sha1_from_bonsai(ctx, cs_id)
            .await?;

        let git = maybegit.ok_or_else(|| anyhow!("git not found for {}", cs_id))?;
        Ok(format!("{}", git))
    } else if to == "bonsai" {
        Ok(format!("{}", cs_id))
    } else {
        return Err(anyhow!("unknown target {}", to));
    }
}

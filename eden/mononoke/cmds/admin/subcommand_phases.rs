/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::format_err;
use anyhow::Error;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use fbinit::FacebookInit;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::str::FromStr;
use std::time::Duration;

use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use context::CoreContext;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;
use phases::PhasesRef;
use slog::info;
use slog::Logger;

use crate::error::SubcommandError;

pub const PHASES: &str = "phases";
const ADD_PUBLIC_PHASES: &str = "add-public";
const FETCH_PHASE: &str = "fetch";
const LIST_PUBLIC: &str = "list-public";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(PHASES)
        .about("commands to work with phases")
        .subcommand(
            SubCommand::with_name(ADD_PUBLIC_PHASES)
                .about("mark mercurial commits as public from provided new-line separated list")
                .arg(
                    Arg::with_name("input-file")
                        .help("new-line separated mercurial public commits")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::with_name("chunk-size")
                        .help("partition input file to chunks of specified size")
                        .long("chunk-size")
                        .takes_value(true),
                ),
        )
        .subcommand(
            SubCommand::with_name(FETCH_PHASE)
                .about("fetch phase of a commit")
                .arg(
                    Arg::with_name("changeset-type")
                        .long("changeset-type")
                        .short("c")
                        .takes_value(true)
                        .possible_values(&["bonsai", "hg"])
                        .required(false)
                        .help(
                            "What changeset type to return, either bonsai or hg. Defaults to hg.",
                        ),
                )
                .arg(
                    Arg::with_name("hash")
                        .help("changeset hash")
                        .takes_value(true),
                ),
        )
        .subcommand(
            SubCommand::with_name(LIST_PUBLIC)
                .arg(
                    Arg::with_name("changeset-type")
                        .long("changeset-type")
                        .short("c")
                        .takes_value(true)
                        .possible_values(&["bonsai", "hg"])
                        .required(false)
                        .help(
                            "What changeset type to return, either bonsai or hg. Defaults to hg.",
                        ),
                )
                .about("List all public commits"),
        )
}

pub async fn subcommand_phases<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'a>,
    sub_m: &'a ArgMatches<'a>,
) -> Result<(), SubcommandError> {
    let repo = args::open_repo(fb, &logger, matches).await?;
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    match sub_m.subcommand() {
        (FETCH_PHASE, Some(sub_m)) => {
            let ty = sub_m.value_of("changeset-type").unwrap_or("hg").to_string();
            let hash = sub_m
                .value_of("hash")
                .map(|s| s.to_string())
                .ok_or(Error::msg("changeset hash is not specified"));

            subcommand_fetch_phase_impl(fb, repo, hash, ty)
                .await
                .map_err(SubcommandError::Error)
        }
        (ADD_PUBLIC_PHASES, Some(sub_m)) => {
            let path = String::from(sub_m.value_of("input-file").unwrap());
            let chunk_size = sub_m
                .value_of("chunk-size")
                .and_then(|chunk_size| chunk_size.parse::<usize>().ok())
                .unwrap_or(16384);

            add_public_phases(ctx, repo, logger, path, chunk_size)
                .await
                .map_err(SubcommandError::Error)
        }
        (LIST_PUBLIC, Some(sub_m)) => {
            let ty = sub_m.value_of("changeset-type").unwrap_or("hg").to_string();

            subcommand_list_public_impl(ctx, ty, repo)
                .await
                .map_err(SubcommandError::Error)
        }
        _ => Err(SubcommandError::InvalidArgs),
    }
}

async fn add_public_phases(
    ctx: CoreContext,
    repo: BlobRepo,
    logger: Logger,
    path: impl AsRef<str>,
    chunk_size: usize,
) -> Result<(), Error> {
    let phases = repo.phases();
    let file = File::open(path.as_ref()).map_err(Error::from)?;
    let hg_changesets = BufReader::new(file)
        .lines()
        .filter_map(|id_str| {
            id_str
                .map_err(Error::from)
                .and_then(|v| HgChangesetId::from_str(&v))
                .ok()
        })
        .map(Ok);
    let mut entries_processed: usize = 0;
    info!(logger, "start processing hashes");
    let mut chunks = stream::iter(hg_changesets)
        .chunks(chunk_size)
        .map(|chunk| chunk.into_iter().collect::<Result<Vec<_>, Error>>());
    while let Some(chunk) = chunks.try_next().await? {
        let count = chunk.len();
        let changesets = repo.get_hg_bonsai_mapping(ctx.clone(), chunk).await?;
        let csids = changesets.into_iter().map(|(_, csid)| csid).collect();
        phases
            .add_public_with_known_public_ancestors(&ctx, csids)
            .await?;
        entries_processed += count;
        print!("\x1b[Khashes processed: {}\r", entries_processed);
        std::io::stdout().flush().expect("flush on stdout failed");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    Ok(())
}

async fn subcommand_list_public_impl(
    ctx: CoreContext,
    ty: String,
    repo: BlobRepo,
) -> Result<(), Error> {
    let public = repo.phases().list_all_public(&ctx).await?;
    if ty == "bonsai" {
        for p in public {
            println!("{}", p);
        }
    } else {
        for chunk in public.chunks(1000) {
            let bonsais: Vec<_> = chunk.to_vec();
            let hg_bonsais = repo.get_hg_bonsai_mapping(ctx.clone(), bonsais).await?;
            let hg_css: Vec<HgChangesetId> = hg_bonsais
                .clone()
                .into_iter()
                .map(|(hg_cs_id, _)| hg_cs_id)
                .collect();

            for hg_cs in hg_css {
                println!("{}", hg_cs);
            }
        }
    }
    Ok(())
}

pub async fn subcommand_fetch_phase_impl<'a>(
    fb: FacebookInit,
    repo: BlobRepo,
    hash: Result<String, Error>,
    ty: String,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let hash = hash?;
    let phases = repo.phases();

    let bcs_id = if ty == "bonsai" {
        ChangesetId::from_str(&hash)?
    } else if ty == "hg" {
        let maybe_bonsai = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, HgChangesetId::from_str(&hash)?)
            .await?;
        maybe_bonsai.ok_or(format_err!("bonsai not found for {}", hash))?
    } else {
        bail!("unknown hash type: {}", ty);
    };

    let public_phases = phases.get_public(&ctx, vec![bcs_id], false).await?;

    if public_phases.contains(&bcs_id) {
        println!("public");
    } else {
        println!("draft");
    }

    Ok(())
}

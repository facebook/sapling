/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use args::MononokeClapApp;
use ascii::AsciiStr;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use clap_old::Arg;
use clap_old::ArgGroup;
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream;
use futures_util::future::TryFutureExt;
use futures_util::stream::StreamExt;
use futures_util::stream::TryStreamExt;
use mercurial_types::HgChangesetId;
use std::fs;
use std::io;
use std::io::BufRead;
use std::path::Path;

fn setup_app<'a, 'b>() -> MononokeClapApp<'a, 'b> {
    args::MononokeAppBuilder::new("Tool to backfill git mappings for given commits")
        .build()
        .arg(Arg::with_name("git").long("git"))
        .arg(Arg::with_name("svnrev").long("svnrev"))
        .group(
            ArgGroup::with_name("mode")
                .args(&["git", "svnrev"])
                .required(true),
        )
        .arg(Arg::from_usage(
            "<IN_FILENAME>  'file with hg changeset ids (separated by newlines)'",
        ))
}

fn parse_input<P: AsRef<Path>>(
    file: P,
) -> Result<impl Iterator<Item = Result<HgChangesetId, Error>>, Error> {
    let file = fs::File::open(file)?;
    let iter = io::BufReader::new(file)
        .lines()
        .map(|line| HgChangesetId::from_ascii_str(AsciiStr::from_ascii(&line?)?));
    Ok(iter)
}

#[derive(Debug, Copy, Clone)]
pub enum BackfillMode {
    Git,
    Svnrev,
}

pub async fn backfill<P: AsRef<Path>>(
    ctx: CoreContext,
    repo: BlobRepo,
    in_path: P,
    mode: BackfillMode,
) -> Result<(), Error> {
    let chunk_size = 1000;
    let ids = parse_input(in_path)?;
    stream::iter(ids)
        .and_then(|hg_cs_id| {
            cloned!(ctx, repo);
            async move {
                let id = repo
                    .bonsai_hg_mapping()
                    .get_bonsai_from_hg(&ctx, hg_cs_id)
                    .await?
                    .ok_or_else(|| anyhow!("hg commit {} is missing", hg_cs_id))?;
                Ok(id)
            }
        })
        .map_ok({
            cloned!(ctx, repo);
            move |id| {
                cloned!(ctx, repo);
                async move { id.load(&ctx, repo.blobstore()).await }.map_err(anyhow::Error::from)
            }
        })
        .try_buffer_unordered(chunk_size)
        // TryStreamExt doesn't have the try_chunks method yet so we have to do it by folding.
        .chunks(chunk_size)
        .map(|chunk| chunk.into_iter().collect::<Result<Vec<_>, _>>())
        .try_for_each(|chunk| {
            cloned!(ctx, repo);
            async move {
                match mode {
                    BackfillMode::Git => {
                        repo.bonsai_git_mapping()
                            .bulk_import_from_bonsai(&ctx, &chunk)
                            .await
                    }
                    BackfillMode::Svnrev => {
                        repo.bonsai_svnrev_mapping()
                            .bulk_import_from_bonsai(&ctx, &chunk)
                            .await
                    }
                }
            }
        })
        .await
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let matches = setup_app().get_matches(fb)?;

    let logger = matches.logger();
    let runtime = matches.runtime();

    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let mode = if matches.is_present("git") {
        BackfillMode::Git
    } else if matches.is_present("svnrev") {
        BackfillMode::Svnrev
    } else {
        panic!("backfill mode not specified");
    };

    let run = async {
        let repo = args::open_repo(fb, logger, &matches).await?;
        let in_filename = matches.value_of("IN_FILENAME").unwrap();
        backfill(ctx, repo, in_filename, mode).await
    };

    runtime.block_on(run)?;
    Ok(())
}

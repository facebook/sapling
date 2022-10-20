/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::io;
use std::io::BufRead;
use std::path::Path;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use ascii::AsciiStr;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use clap::ArgGroup;
use clap::Parser;
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream;
use futures_util::future::TryFutureExt;
use futures_util::stream::StreamExt;
use futures_util::stream::TryStreamExt;
use mercurial_types::HgChangesetId;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeAppBuilder;

#[derive(Parser)]
#[clap(about = "Tool to backfill git mappings for given commits")]
#[clap(group(
    ArgGroup::new("mode")
        .required(true)
        .args(&["git", "svnrev"]),
))]

struct BackFillArgs {
    #[clap(flatten)]
    repo: RepoArgs,
    #[clap(long, action)]
    git: bool,
    #[clap(long, action)]
    svnrev: bool,
    #[clap(
        value_parser,
        help = " file with hg changeset ids (separated by newlines) "
    )]
    in_filename: Option<String>,
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
    let app = MononokeAppBuilder::new(fb).build::<BackFillArgs>()?;

    let args: BackFillArgs = app.args()?;

    let logger = app.logger();
    let runtime = app.runtime();

    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let mode = if args.git {
        BackfillMode::Git
    } else if args.svnrev {
        BackfillMode::Svnrev
    } else {
        panic!("backfill mode not specified");
    };

    let run = async {
        let repo = app
            .open_repo(&args.repo)
            .await
            .context("Failed to open repo")?;
        let in_filename = args.in_filename.unwrap();
        backfill(ctx, repo, in_filename, mode).await
    };

    runtime.block_on(run)?;
    Ok(())
}

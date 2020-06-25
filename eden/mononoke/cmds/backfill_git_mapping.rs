/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use ascii::AsciiStr;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use clap::{App, Arg};
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{compat::Future01CompatExt, stream};
use futures_util::future::TryFutureExt;
use futures_util::stream::{StreamExt, TryStreamExt};
use mercurial_types::HgChangesetId;
use std::fs;
use std::io::{self, BufRead};
use std::path::Path;

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    args::MononokeApp::new("Tool to backfill git mappings for given commits")
        .build()
        .version("0.0.0")
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

pub async fn backfill<P: AsRef<Path>>(
    ctx: CoreContext,
    repo: BlobRepo,
    in_path: P,
) -> Result<(), Error> {
    let chunk_size = 1000;
    let ids = parse_input(in_path)?;
    stream::iter(ids)
        .and_then(|hg_cs_id| {
            cloned!(ctx, repo);
            async move {
                let id = repo
                    .get_bonsai_from_hg(ctx.clone(), hg_cs_id)
                    .compat()
                    .await?
                    .ok_or(anyhow!("hg commit {} is missing", hg_cs_id))?;
                Ok(id)
            }
        })
        .map_ok({
            cloned!(ctx, repo);
            move |id| {
                id.load(ctx.clone(), repo.blobstore())
                    .map_err(anyhow::Error::from)
            }
        })
        .try_buffer_unordered(chunk_size)
        // TryStreamExt doesn't have the try_chunks method yet so we have to do it by folding.
        .chunks(chunk_size)
        .map(|chunk| chunk.into_iter().collect::<Result<Vec<_>, _>>())
        .try_for_each(|chunk| {
            cloned!(ctx, repo);
            async move {
                repo.bonsai_git_mapping()
                    .bulk_import_from_bonsai(&ctx, &chunk)
                    .await
            }
        })
        .await
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let matches = setup_app().get_matches();

    let (_, logger, mut runtime) = args::init_mononoke(fb, &matches, None)?;

    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let run = async {
        let repo = args::open_repo(fb, &logger, &matches).compat().await?;
        let in_filename = matches.value_of("IN_FILENAME").unwrap();
        backfill(ctx, repo, in_filename).await
    };

    runtime.block_on_std(run)?;
    Ok(())
}

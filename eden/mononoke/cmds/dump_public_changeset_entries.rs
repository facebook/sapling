/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use blobrepo::BlobRepo;
use bulkops::{Direction, PublicChangesetBulkFetch};
use changesets::serialize_cs_entries;
use clap::Arg;
use cmdlib::args::{self, RepoRequirement};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::TryStreamExt;
use phases::PhasesArc;

const ARG_OUT_FILENAME: &str = "out-filename";

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeAppBuilder::new("Dump all public changeset entries to a file")
        .with_advanced_args_hidden()
        .with_fb303_args()
        .with_repo_required(RepoRequirement::AtLeastOne)
        .build()
        .about(
            "Utility to write public changeset for a given repo to a file. \
            It can be used by other tools that want to avoid an expensive prefetching.",
        )
        .arg(
            Arg::with_name(ARG_OUT_FILENAME)
                .long(ARG_OUT_FILENAME)
                .takes_value(true)
                .required(true)
                .help("file name where commits will be saved"),
        );
    let matches = app.get_matches(fb)?;
    let runtime = matches.runtime();
    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let out_filename = matches
        .value_of(ARG_OUT_FILENAME)
        .ok_or_else(|| anyhow!("missing required argument: {}", ARG_OUT_FILENAME))?
        .to_string();
    let blob_repo_fut = args::open_repo(fb, &logger, &matches);

    runtime.block_on(async move {
        let repo: BlobRepo = blob_repo_fut.await?;

        let fetcher =
            PublicChangesetBulkFetch::new(repo.get_changesets_object(), repo.phases_arc());

        let css = fetcher
            .fetch(&ctx, Direction::OldestFirst)
            .try_collect()
            .await?;

        let serialized = serialize_cs_entries(css);
        tokio::fs::write(out_filename, serialized).await?;

        Ok(())
    })
}

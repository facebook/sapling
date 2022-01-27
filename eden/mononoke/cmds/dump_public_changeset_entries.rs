/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Result};
use blobrepo::BlobRepo;
use bulkops::{Direction, PublicChangesetBulkFetch};
use bytes::Bytes;
use changesets::{deserialize_cs_entries, serialize_cs_entries};
use clap::{Arg, ArgGroup};
use cmdlib::args::{self, RepoRequirement};
use cmdlib::helpers::csid_resolve;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::TryStreamExt;
use mononoke_types::ChangesetId;
use phases::PhasesArc;
use std::path::Path;

const ARG_OUT_FILENAME: &str = "out-filename";
const ARG_START_COMMIT: &str = "start-commit";
const ARG_START_FROM_FILE_END: &str = "start-from-file-end";

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
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
        )
        .arg(
            Arg::with_name(ARG_START_COMMIT)
                .long(ARG_START_COMMIT)
                .takes_value(true)
                .help("start fetching from this commit rather than the beginning of time"),
        )
        .arg(
            Arg::with_name(ARG_START_FROM_FILE_END)
                .long(ARG_START_FROM_FILE_END)
                .takes_value(true)
                .help("start fetching from the last commit in this file, for incremental updates"),
        )
        .group(
            ArgGroup::with_name("starting-commit")
                .args(&[ARG_START_COMMIT, ARG_START_FROM_FILE_END]),
        );
    let matches = app.get_matches(fb)?;
    let runtime = matches.runtime();
    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let out_filename = matches
        .value_of(ARG_OUT_FILENAME)
        .ok_or_else(|| anyhow!("missing required argument: {}", ARG_OUT_FILENAME))?
        .to_string();

    let opt_start_file = matches.value_of_os(ARG_START_FROM_FILE_END);
    let opt_start_commit = matches.value_of(ARG_START_COMMIT);

    let blob_repo_fut = args::open_repo(fb, &logger, &matches);

    runtime.block_on(async move {
        let repo: BlobRepo = blob_repo_fut.await?;

        let fetcher =
            PublicChangesetBulkFetch::new(repo.get_changesets_object(), repo.phases_arc());

        let start_commit = {
            if let Some(path) = opt_start_file {
                load_last_commit(path.as_ref()).await?
            } else if let Some(start_commit) = opt_start_commit {
                Some(csid_resolve(&ctx, &repo, start_commit).await?)
            } else {
                None
            }
        };

        let bounds = fetcher
            .get_repo_bounds_after_commits(&ctx, start_commit.into_iter().collect())
            .await?;
        let css = fetcher
            .fetch_bounded(&ctx, Direction::OldestFirst, Some(bounds))
            .try_collect()
            .await?;

        let serialized = serialize_cs_entries(css);
        tokio::fs::write(out_filename, serialized).await?;

        Ok(())
    })
}

async fn load_last_commit(filename: &Path) -> Result<Option<ChangesetId>> {
    let file_contents = Bytes::from(tokio::fs::read(filename).await?);
    let cs_entries = deserialize_cs_entries(&file_contents)?;
    Ok(cs_entries.last().map(|e| e.cs_id))
}

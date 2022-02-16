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
use changesets::{deserialize_cs_entries, serialize_cs_entries, ChangesetEntry};
use clap_old::{Arg, ArgGroup};
use cmdlib::args::{self, RepoRequirement};
use cmdlib::helpers::csid_resolve;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{future, stream, StreamExt, TryStreamExt};
use mononoke_types::ChangesetId;
use phases::PhasesArc;
use std::path::Path;

const ARG_OUT_FILENAME: &str = "out-filename";
const ARG_START_COMMIT: &str = "start-commit";
const ARG_START_FROM_FILE_END: &str = "start-from-file-end";
const ARG_MERGE_FILE: &str = "merge-file";

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
        .arg(
            Arg::with_name(ARG_MERGE_FILE)
                .long(ARG_MERGE_FILE)
                .takes_value(true)
                .multiple(true)
                .help(
                    "Merge commits from this file into the final output. User is responsible for \
                    avoiding duplicate commits between files and database fetch. Can be repeated",
                ),
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
    let opt_merge_files = matches.values_of_os(ARG_MERGE_FILE);
    let merge_files = opt_merge_files
        .into_iter()
        .flatten()
        .map(|path| load_file_contents(path.as_ref()));

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

        let css = {
            let (mut file_css, db_css): (Vec<_>, Vec<_>) = future::try_join(
                stream::iter(merge_files).buffered(2).try_concat(),
                fetcher
                    .fetch_bounded(&ctx, Direction::OldestFirst, Some(bounds))
                    .try_collect::<Vec<_>>(),
            )
            .await?;
            file_css.extend(db_css.into_iter());
            file_css
        };

        let serialized = serialize_cs_entries(css);
        tokio::fs::write(out_filename, serialized).await?;

        Ok(())
    })
}

async fn load_file_contents(filename: &Path) -> Result<Vec<ChangesetEntry>> {
    let file_contents = Bytes::from(tokio::fs::read(filename).await?);
    deserialize_cs_entries(&file_contents)
}

async fn load_last_commit(filename: &Path) -> Result<Option<ChangesetId>> {
    Ok(load_file_contents(filename).await?.last().map(|e| e.cs_id))
}

/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use borrowed::borrowed;
use clap::Arg;
use cmdlib::{
    args::{self, MononokeMatches},
    helpers,
};
use context::CoreContext;
use derived_data::BonsaiDerived;
use fbinit::FacebookInit;
use futures::{stream, StreamExt, TryStreamExt};
use manifest::{Entry, ManifestOps};
use mononoke_types::ChangesetId;
use serde::Serialize;
use skeleton_manifest::RootSkeletonManifestId;

const ARG_IN_FILE: &str = "input-file";

async fn run<'a>(fb: FacebookInit, matches: &'a MononokeMatches<'a>) -> Result<(), Error> {
    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let repo: BlobRepo = args::open_repo(fb, ctx.logger(), &matches).await?;

    let input_file = matches
        .value_of(ARG_IN_FILE)
        .ok_or_else(|| anyhow!("{} not set", ARG_IN_FILE))?;

    let csids = helpers::csids_resolve_from_file(&ctx, &repo, input_file).await?;

    borrowed!(ctx, repo);
    let commit_stats = stream::iter(csids)
        .map(|cs_id| async move { find_commit_stat(&ctx, &repo, cs_id).await })
        .buffered(100)
        .try_collect::<Vec<_>>()
        .await?;


    println!("{}", serde_json::to_string_pretty(&commit_stats)?);

    Ok(())
}

#[derive(Serialize)]
struct CommitStat {
    cs_id: ChangesetId,
    largest_touched_dir_size: u64,
    largest_touched_dir_name: String,
    num_changed_files: u64,
    sum_of_sizes_of_all_changed_directories: u64,
}

async fn find_commit_stat(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<CommitStat, Error> {
    let bcs = cs_id.load(ctx, repo.blobstore()).await?;
    let mut paths = vec![];
    for (path, _) in bcs.file_changes() {
        paths.extend(path.clone().into_parent_dir_iter());
    }
    let root_skeleton_id = RootSkeletonManifestId::derive(&ctx, repo, cs_id).await?;
    let entries = root_skeleton_id
        .skeleton_manifest_id()
        .find_entries(ctx.clone(), repo.get_blobstore(), paths)
        .try_filter_map(|(path, entry)| async move {
            let tree = match entry {
                Entry::Tree(tree_id) => Some((path, tree_id)),
                Entry::Leaf(_) => None,
            };
            Ok(tree)
        })
        .map_ok(|(path, tree_id)| async move {
            let entry = tree_id.load(ctx, &repo.get_blobstore()).await?;
            Ok((path, entry.list().collect::<Vec<_>>().len()))
        })
        .try_buffer_unordered(100)
        .try_collect::<Vec<_>>()
        .await?;

    let mut sum_of_sizes_of_all_changed_directories: u64 = 0;
    for (_, size) in &entries {
        sum_of_sizes_of_all_changed_directories += (*size) as u64;
    }

    let (largest_touched_dir_size, largest_touched_dir_name) = entries
        .into_iter()
        .max_by_key(|(_, size)| *size)
        .map(|(path, size)| (size as u64, path))
        .unwrap_or_else(|| (0, None));

    let largest_touched_dir_name = match largest_touched_dir_name {
        Some(dir_name) => {
            format!("{}", dir_name)
        }
        None => "root".to_string(),
    };
    let stat = CommitStat {
        cs_id,
        largest_touched_dir_size,
        largest_touched_dir_name,
        num_changed_files: bcs.file_changes_map().len() as u64,
        sum_of_sizes_of_all_changed_directories,
    };

    Ok(stat)
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let matches = args::MononokeAppBuilder::new("Binary that can compute stats about commits")
        .with_advanced_args_hidden()
        .build()
        .about("A tool to collect different stat about commits")
        .arg(
            Arg::with_name(ARG_IN_FILE)
                .long(ARG_IN_FILE)
                .required(true)
                .takes_value(true)
                .help("Filename with commit hashes or bookmarks"),
        )
        .get_matches(fb)?;

    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run(fb, &matches))
}

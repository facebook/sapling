/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use borrowed::borrowed;
use bulkops::Direction;
use bulkops::PublicChangesetBulkFetch;
use changesets::ChangesetsArc;
use clap_old::Arg;
use clap_old::ArgGroup;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use context::CoreContext;
use derived_data::BonsaiDerived;
use fbinit::FacebookInit;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_types::ChangesetId;
use phases::PhasesArc;
use serde::Serialize;
use skeleton_manifest::RootSkeletonManifestId;

const ARG_IN_FILE: &str = "input-file";
const ARG_ALL_COMMITS: &str = "all-commits-in-repo";

async fn run<'a>(fb: FacebookInit, matches: &'a MononokeMatches<'a>) -> Result<(), Error> {
    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let repo: BlobRepo =
        args::not_shardmanager_compatible::open_repo(fb, ctx.logger(), matches).await?;
    let fetcher;

    let csids = {
        match (
            matches.value_of(ARG_IN_FILE),
            matches.is_present(ARG_ALL_COMMITS),
        ) {
            (Some(input_file), false) => stream::iter(
                helpers::csids_resolve_from_file(&ctx, &repo, input_file)
                    .await?
                    .into_iter()
                    .map(Ok::<_, Error>),
            )
            .left_stream(),
            (None, true) => {
                fetcher = PublicChangesetBulkFetch::new(repo.changesets_arc(), repo.phases_arc());
                fetcher
                    .fetch_ids(&ctx, Direction::OldestFirst, None)
                    .map_ok(|((cs_id, _bound), _fetch_bounds)| cs_id)
                    .right_stream()
            }
            (None, false) => bail!("Neither {} nor {} set", ARG_IN_FILE, ARG_ALL_COMMITS),
            (Some(_), true) => bail!("Both {} nor {} set", ARG_IN_FILE, ARG_ALL_COMMITS),
        }
    };

    borrowed!(ctx, repo);
    let commit_stats = csids
        .map_ok(|cs_id| async move { find_commit_stat(ctx, repo, cs_id).await })
        .try_buffered(100)
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
    copy_froms: u64,
}

async fn find_commit_stat(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<CommitStat, Error> {
    let bcs = cs_id.load(ctx, repo.blobstore()).await?;
    let mut paths = vec![];
    let mut copy_froms = 0;
    for (path, file_change) in bcs.file_changes() {
        paths.extend(path.clone().into_parent_dir_iter());
        if file_change.copy_from().is_some() {
            copy_froms += 1;
        }
    }

    let root_skeleton_id = RootSkeletonManifestId::derive(ctx, repo, cs_id).await?;
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
        .map_or((0, None), |(path, size)| (size as u64, path));

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
        copy_froms,
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
                .takes_value(true)
                .help("Filename with commit hashes or bookmarks"),
        )
        .arg(
            Arg::with_name(ARG_ALL_COMMITS)
                .long(ARG_ALL_COMMITS)
                .takes_value(false)
                .help("Examine all public commits in this repo"),
        )
        .group(
            ArgGroup::with_name("source")
                .args(&[ARG_IN_FILE, ARG_ALL_COMMITS])
                .required(true),
        )
        .get_matches(fb)?;

    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run(fb, &matches))
}

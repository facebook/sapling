/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use context::CoreContext;
use megarepolib::chunking::even_chunker_with_max_size;
use megarepolib::chunking::parse_chunking_hint;
use megarepolib::chunking::path_chunker_from_hint;
use megarepolib::pre_merge_delete::PreMergeDelete;
use megarepolib::pre_merge_delete::create_pre_merge_delete;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::RepoArgs;
use slog::info;

use super::common::LightResultingChangesetArgs;
// use super::common::get_commit_factory;
use super::common::get_delete_commits_cs_args_factory;

/// Create a merge commit with given parents
#[derive(Debug, clap::Args)]
pub struct PreMergeDeleteArgs {
    #[clap(flatten)]
    pub repo_args: RepoArgs,

    /// Commit from which to start deletion and optionally a commit that will be
    /// diffed against to find what files needs to be deleted - only files that
    /// don't exist or differ from base commit will be deleted.
    #[clap(flatten)]
    pub commits: ChangesetArgs,

    #[command(flatten)]
    pub res_cs_args: LightResultingChangesetArgs,

    /// A path to working copy chunking hint. If not provided, working copy will
    /// be chunked evenly into `--even-chunk-size` commits
    #[clap(long)]
    pub chunking_hint_file: Option<String>,

    /// Chunk size for even chunking when --chunking-hing-file is not provided
    #[clap(long)]
    pub even_chunk_size: Option<usize>,
}

pub async fn run(ctx: &CoreContext, app: MononokeApp, args: PreMergeDeleteArgs) -> Result<()> {
    let repo: Repo = app.open_repo(&args.repo_args).await?;

    let delete_cs_args_factory = get_delete_commits_cs_args_factory(args.res_cs_args)?;

    let chunker = match args.chunking_hint_file {
        Some(hint_file) => {
            let hint_str = std::fs::read_to_string(hint_file)?;
            let hint = parse_chunking_hint(hint_str)?;
            path_chunker_from_hint(hint)?
        }
        None => {
            let even_chunk_size: usize = args.even_chunk_size.ok_or_else(|| {
                anyhow!("either chunking_hint_file or even_chunk_size is required")
            })?;
            even_chunker_with_max_size(even_chunk_size)?
        }
    };

    let changesets = args.commits.resolve_changesets(ctx, &repo).await?;
    let (parent_bcs_id, base_bcs_id) = match changesets[..] {
        [parent_bcs_id, base_bcs_id] => (parent_bcs_id, Some(base_bcs_id)),
        [parent_bcs_id] => (parent_bcs_id, None),
        _ => bail!("expected 1 or 2 commits, got {}", changesets.len()),
    };

    let pmd = create_pre_merge_delete(
        ctx,
        &repo,
        parent_bcs_id,
        chunker,
        delete_cs_args_factory.as_ref(),
        base_bcs_id,
    )
    .await?;

    let PreMergeDelete { mut delete_commits } = pmd;

    info!(
        ctx.logger(),
        "Listing deletion commits in top-to-bottom order (first commit is a descendant of the last)"
    );
    delete_commits.reverse();
    for delete_commit in delete_commits {
        println!("{}", delete_commit);
    }

    Ok(())
}

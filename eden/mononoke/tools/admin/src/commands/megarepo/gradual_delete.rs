/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use context::CoreContext;
use megarepolib::chunking::even_chunker_with_max_size;
use megarepolib::common::delete_files_in_chunks;
use megarepolib::working_copy::get_working_copy_paths_by_prefixes;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::RepoArgs;
use mononoke_types::NonRootMPath;
use slog::info;

use super::common::LightResultingChangesetArgs;
use crate::commands::megarepo::common::get_delete_commits_cs_args_factory;

#[derive(Debug, clap::Args)]
pub struct GradualDeleteArgs {
    #[clap(flatten)]
    pub repo_args: RepoArgs,

    /// Commit from which to start deletion
    #[clap(flatten)]
    pub commit: ChangesetArgs,

    #[clap(flatten)]
    pub res_cs_args: LightResultingChangesetArgs,

    /// Chunk size for even chunking
    #[clap(long)]
    pub even_chunk_size: usize,

    /// Paths to delete
    #[clap(required = true)]
    pub paths: Vec<String>,
}

pub async fn run(ctx: &CoreContext, app: MononokeApp, args: GradualDeleteArgs) -> Result<()> {
    let repo: Repo = app.open_repo(&args.repo_args).await?;

    let delete_cs_args_factory = get_delete_commits_cs_args_factory(args.res_cs_args)?;
    let chunker = even_chunker_with_max_size(args.even_chunk_size)?;

    let parent_bcs_id = args.commit.resolve_changeset(ctx, &repo).await?;

    let path_prefixes = args
        .paths
        .into_iter()
        .map(NonRootMPath::new)
        .collect::<Result<Vec<_>>>()?;

    info!(
        ctx.logger(),
        "Gathering working copy files under {:?}", path_prefixes
    );
    let paths =
        get_working_copy_paths_by_prefixes(ctx, &repo, parent_bcs_id, path_prefixes).await?;
    info!(ctx.logger(), "{} paths to be deleted", paths.len());

    info!(ctx.logger(), "Starting deletion");
    let delete_commits = delete_files_in_chunks(
        ctx,
        &repo,
        parent_bcs_id,
        paths,
        &chunker,
        &delete_cs_args_factory,
        false, /* skip_last_chunk */
    )
    .await?;

    info!(ctx.logger(), "Deletion finished");
    info!(
        ctx.logger(),
        "Listing commits in an ancestor-descendant order"
    );
    for delete_commit in delete_commits {
        println!("{}", delete_commit);
    }

    Ok(())
}

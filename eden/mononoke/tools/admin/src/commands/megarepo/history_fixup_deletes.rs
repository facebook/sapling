/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::Result;
use anyhow::anyhow;
use commit_id::CommitIdNames;
use commit_id::NamedCommitIdsArgs;
use commit_id::resolve_commit_id;
use context::CoreContext;
use megarepolib::chunking::even_chunker_with_max_size;
use megarepolib::history_fixup_delete::HistoryFixupDeletes;
use megarepolib::history_fixup_delete::create_history_fixup_deletes;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use mononoke_types::path::NonRootMPath;
use slog::info;
use tokio::fs::read_to_string;

use super::common::LightResultingChangesetArgs;
use crate::commands::megarepo::common::get_delete_commits_cs_args_factory;

#[derive(Copy, Clone, Debug)]
struct HistoryFixupDeletesCommitIdNames;

impl CommitIdNames for HistoryFixupDeletesCommitIdNames {
    const NAMES: &'static [(&'static str, &'static str)] = &[
        (
            "fixup-commit",
            "Commit which we want to fixup (the files specified in paths file will be deleted there)",
        ),
        (
            "correct-history-commit",
            "Commit containing the files with correct history (the files specified in path files will be preserved there; all the other files will be deleted)",
        ),
    ];
}

/// Create a set of delete commits before the path fixup.
#[derive(Debug, clap::Args)]
pub struct HistoryFixupDeletesArgs {
    #[clap(flatten)]
    pub repo_args: RepoArgs,

    #[clap(flatten)]
    res_cs_args: LightResultingChangesetArgs,

    #[clap(flatten)]
    commit_ids: NamedCommitIdsArgs<HistoryFixupDeletesCommitIdNames>,

    /// Chunk size for even chunking
    #[clap(long)]
    pub even_chunk_size: usize,

    /// File containing paths to fixup separated by newlines
    #[clap(long)]
    paths_file: PathBuf,
}

pub async fn run(ctx: &CoreContext, app: MononokeApp, args: HistoryFixupDeletesArgs) -> Result<()> {
    let repo: Repo = app.open_repo(&args.repo_args).await?;

    let delete_cs_args_factory = get_delete_commits_cs_args_factory(args.res_cs_args)?;
    let chunker = even_chunker_with_max_size(args.even_chunk_size)?;

    let named_commit_ids = &args.commit_ids.named_commit_ids();
    let fixup_csid = resolve_commit_id(
        ctx,
        &repo,
        named_commit_ids
            .get("fixup-commit")
            .ok_or(anyhow!("fixup-commit is required"))?,
    )
    .await?;
    let correct_csid = resolve_commit_id(
        ctx,
        &repo,
        named_commit_ids
            .get("correct-history-commit")
            .ok_or(anyhow!("correct-history-commit is required"))?,
    )
    .await?;

    let s = read_to_string(args.paths_file).await?;
    let paths: Vec<NonRootMPath> = s
        .lines()
        .map(NonRootMPath::new)
        .collect::<Result<Vec<NonRootMPath>>>()?;
    let hfd = create_history_fixup_deletes(
        ctx,
        &repo,
        fixup_csid,
        chunker,
        delete_cs_args_factory,
        correct_csid,
        paths,
    )
    .await?;

    let HistoryFixupDeletes {
        mut delete_commits_fixup_branch,
        mut delete_commits_correct_branch,
    } = hfd;

    info!(
        ctx.logger(),
        "Listing deletion commits for fixup branch in top-to-bottom order (first commit is a descendant of the last)"
    );
    delete_commits_fixup_branch.reverse();
    for delete_commit in delete_commits_fixup_branch {
        println!("{}", delete_commit);
    }

    info!(
        ctx.logger(),
        "Listing deletion commits for branch with correct history in top-to-bottom order (first commit is a descendant of the last)"
    );
    delete_commits_correct_branch.reverse();
    for delete_commit in delete_commits_correct_branch {
        println!("{}", delete_commit);
    }

    Ok(())
}

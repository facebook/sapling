/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::Result;
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

// TODO: Delete this mod after D73586055 lands
mod fake_commit_id {
    use anyhow::Result;
    use bonsai_git_mapping::BonsaiGitMappingRef;
    use bonsai_globalrev_mapping::BonsaiGlobalrevMappingRef;
    use bonsai_hg_mapping::BonsaiHgMappingRef;
    use bonsai_svnrev_mapping::BonsaiSvnrevMappingRef;
    use bookmarks::BookmarkKey;
    use bookmarks::BookmarksRef;
    use commit_id::parse_commit_id;
    use context::CoreContext;
    use mononoke_types::ChangesetId;

    pub trait Repo = BonsaiHgMappingRef
        + BonsaiGitMappingRef
        + BonsaiGlobalrevMappingRef
        + BonsaiSvnrevMappingRef;

    pub async fn parse_commit(
        ctx: &CoreContext,
        repo: &(impl Repo + BookmarksRef),
        hash_or_bookmark: &str,
    ) -> Result<ChangesetId> {
        let hash_or_bookmark = hash_or_bookmark.to_string();
        if let Ok(name) = BookmarkKey::new(hash_or_bookmark.clone()) {
            if let Some(cs_id) = repo.bookmarks().get(ctx.clone(), &name).await? {
                return Ok(cs_id);
            }
        }
        parse_commit_id(ctx, repo, &hash_or_bookmark).await
    }
}
// TODO: Replace with `use commit_id::parse_commit;` after D73586055 lands
use fake_commit_id::parse_commit;

/// Create a set of delete commits before the path fixup.
#[derive(Debug, clap::Args)]
pub struct HistoryFixupDeletesArgs {
    #[clap(flatten)]
    pub repo_args: RepoArgs,

    #[clap(flatten)]
    res_cs_args: LightResultingChangesetArgs,

    /// Commit which we want to fixup (the files specified in paths file will be deleted there)
    #[clap(long)]
    commit: String,

    /// Commit containing the files with correct history (the files specified in path files will be preserved there; all the other files will be deleted)
    #[clap(long)]
    commit_correct_history: String,

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

    let fixup_csid = parse_commit(ctx, &repo, &args.commit).await?;
    let correct_csid = parse_commit(ctx, &repo, &args.commit_correct_history).await?;

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

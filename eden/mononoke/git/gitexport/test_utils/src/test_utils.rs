/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::anyhow;
use anyhow::Error;
use fbinit::FacebookInit;
use futures::future::try_join_all;
use maplit::hashmap;
use mononoke_api::ChangesetContext;
use mononoke_api::CoreContext;
use mononoke_api::MononokeError;
use mononoke_api::RepoContext;
use mononoke_types::ChangesetId;
use test_repo_factory::TestRepoFactory;
use tests_utils::bookmark;
use tests_utils::CreateCommitContext;

// Directory and file constants.
// By convention, directories with uppercase names are exported.

pub const EXPORT_DIR: &str = "A";
pub const EXPORT_FILE: &str = "A/bar.txt";
pub const SECOND_EXPORT_FILE: &str = "A/foo.txt";

pub const IRRELEVANT_FILE: &str = "b/bar.txt";
pub const SECOND_IRRELEVANT_FILE: &str = "b/foo.txt";

pub const SECOND_EXPORT_DIR: &str = "C";
pub const FILE_IN_SECOND_EXPORT_DIR: &str = "C/foo.txt";

pub async fn get_relevant_changesets_from_ids(
    repo_ctx: &RepoContext,
    cs_ids: Vec<ChangesetId>,
) -> Result<Vec<ChangesetContext>, MononokeError> {
    try_join_all(cs_ids.iter().map(|cs_id| async {
        let csc: ChangesetContext = repo_ctx
            .changeset(*cs_id)
            .await?
            .ok_or(anyhow!("Can't get ChangesetContext from id"))?;
        Ok::<ChangesetContext, MononokeError>(csc)
    }))
    .await
}

#[derive(Default)]
pub struct GitExportTestRepoOptions {
    pub add_branch_commit: bool,
}

pub async fn build_test_repo(
    fb: FacebookInit,
    ctx: &CoreContext,
    opts: GitExportTestRepoOptions,
) -> Result<(RepoContext, HashMap<String, ChangesetId>), Error> {
    let source_repo = TestRepoFactory::new(fb)?.build().await?;
    let source_repo_ctx = RepoContext::new_test(ctx.clone(), source_repo).await?;
    let source_repo = source_repo_ctx.repo();

    // Add file to export directory -> full export
    let first = CreateCommitContext::new_root(ctx, source_repo)
        .add_file(EXPORT_FILE, "file_to_export")
        .set_message("first")
        .commit()
        .await?;

    // Add an irrelevant file -> not exported
    let second = CreateCommitContext::new(ctx, source_repo, vec![first])
        .add_file(IRRELEVANT_FILE, "IRRELEVANT_FILE")
        .set_message("second")
        .commit()
        .await?;

    // Modify relevant and irrelevant files -> partial export
    let third = CreateCommitContext::new(ctx, source_repo, vec![second])
        .add_file(EXPORT_FILE, "change EXPORT_FILE")
        .add_file(IRRELEVANT_FILE, "change IRRELEVANT_FILE")
        .set_message("third")
        .commit()
        .await?;

    // Modify only irrelevant files -> not exported
    let fourth = CreateCommitContext::new(ctx, source_repo, vec![third])
        .add_file(IRRELEVANT_FILE, "change only IRRELEVANT_FILE")
        .set_message("fourth")
        .commit()
        .await?;

    // Modify only relevant files -> full export
    let fifth = CreateCommitContext::new(ctx, source_repo, vec![fourth])
        .add_file(EXPORT_FILE, "change only EXPORT_FILE in fifth")
        .set_message("fifth")
        .commit()
        .await?;

    // Add relevant and irrelevant files -> partial export
    let sixth = CreateCommitContext::new(ctx, source_repo, vec![fifth])
        .add_file(
            FILE_IN_SECOND_EXPORT_DIR,
            "Create file in second export dir",
        )
        .add_file(SECOND_IRRELEVANT_FILE, "SECOND_IRRELEVANT_FILE")
        .set_message("sixth")
        .commit()
        .await?;

    let mut maybe_branch_commit = None;
    let mut merge_commit_parents = vec![sixth];

    if opts.add_branch_commit {
        // Create a commit that will branch from the fifth and merge into the seventh

        let branch_commit = CreateCommitContext::new(ctx, source_repo, vec![fifth])
            .add_file(SECOND_EXPORT_FILE, "change export_file in a branch")
            .set_message("branch commit")
            .commit()
            .await?;

        maybe_branch_commit = Some(branch_commit);
        merge_commit_parents.push(branch_commit);
    }

    // Change both relevant files -> full export
    let seventh = CreateCommitContext::new(ctx, source_repo, merge_commit_parents)
        .add_file(EXPORT_FILE, "change export file again")
        .add_file(
            FILE_IN_SECOND_EXPORT_DIR,
            "change file in second export dir again",
        )
        .set_message("seventh")
        .commit()
        .await?;

    // Delete irrelevant file -> not exported
    let eighth = CreateCommitContext::new(ctx, source_repo, vec![seventh])
        .delete_file(IRRELEVANT_FILE)
        .set_message("eighth")
        .commit()
        .await?;

    // Delete relevant file -> full export
    let ninth = CreateCommitContext::new(ctx, source_repo, vec![eighth])
        .delete_file(SECOND_EXPORT_FILE)
        .set_message("ninth")
        .commit()
        .await?;

    let tenth = CreateCommitContext::new(ctx, source_repo, vec![ninth])
        .add_file(EXPORT_FILE, "add export file back")
        .set_message("tenth")
        .commit()
        .await?;

    let bookmark_update_ctx = bookmark(ctx, source_repo, "master");
    let _master_bookmark_key = bookmark_update_ctx.set_to(tenth.clone()).await?;

    let mut cs_map = hashmap! {
        String::from("first") => first,
        String::from("second") => second,
        String::from("third") => third,
        String::from("fourth") => fourth,
        String::from("fifth") => fifth,
        String::from("sixth") => sixth,
        String::from("seventh") => seventh,
        String::from("eighth") => eighth,
        String::from("ninth") => ninth,
        String::from("tenth") => tenth,
    };

    if let Some(branch_commit) = maybe_branch_commit {
        cs_map.insert(String::from("branch"), branch_commit);
    }

    Ok((source_repo_ctx, cs_map))
}

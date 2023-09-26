/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use anyhow::anyhow;
use anyhow::Error;
use fbinit::FacebookInit;
use futures::future::try_join_all;
use mononoke_api::ChangesetContext;
use mononoke_api::CoreContext;
use mononoke_api::MononokeError;
use mononoke_api::RepoContext;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use test_repo_factory::TestRepoFactory;
use tests_utils::bookmark;
use tests_utils::drawdag::changes;
use tests_utils::drawdag::create_from_dag_with_changes;

// Directory and file constants.
// By convention, directories with uppercase names are exported.

pub const EXPORT_DIR: &str = "EXP";
pub const EXPORT_FILE: &str = "EXP/bar.txt";
pub const SECOND_EXPORT_FILE: &str = "EXP/foo.txt";

pub const IRRELEVANT_FILE: &str = "internal/bar.txt";
pub const SECOND_IRRELEVANT_FILE: &str = "internal/foo.txt";

pub const SECOND_EXPORT_DIR: &str = "EXP_2";
pub const FILE_IN_SECOND_EXPORT_DIR: &str = "EXP_2/foo.txt";

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
) -> Result<(RepoContext, BTreeMap<String, ChangesetId>), Error> {
    let source_repo = TestRepoFactory::new(fb)?.build().await?;
    let source_repo_ctx = RepoContext::new_test(ctx.clone(), source_repo).await?;
    let source_repo = source_repo_ctx.repo();

    let mut dag_changes = changes! {
        "A" => |c| c.add_file(EXPORT_FILE, "file_to_export")
        .set_author_date(DateTime::from_timestamp(1000, 0).unwrap()),
        "B" => |c| c.add_file(IRRELEVANT_FILE, "IRRELEVANT_FILE")
        .set_author_date(DateTime::from_timestamp(2000, 0).unwrap()),
        "C" => |c| c.add_file(EXPORT_FILE, "change EXPORT_FILE")
        .add_file(IRRELEVANT_FILE, "change IRRELEVANT_FILE")
        .set_author_date(DateTime::from_timestamp(3000, 0).unwrap()),
        "D" => |c| c .add_file(IRRELEVANT_FILE, "change only IRRELEVANT_FILE")
        .set_author_date(DateTime::from_timestamp(4000, 0).unwrap()),
        "E" => |c| c.add_file(EXPORT_FILE, "change only EXPORT_FILE in fifth")
        .set_author_date(DateTime::from_timestamp(5000, 0).unwrap()),
        "F" => |c| c.add_file(
            FILE_IN_SECOND_EXPORT_DIR,
            "Create file in second export dir",
        )
        .add_file(SECOND_IRRELEVANT_FILE, "SECOND_IRRELEVANT_FILE")
        .set_author_date(DateTime::from_timestamp(6000, 0).unwrap()),
        "G" => |c| c.add_file(EXPORT_FILE, "change export file again")
        .add_file(
            FILE_IN_SECOND_EXPORT_DIR,
            "change file in second export dir again",
        )
        .set_author_date(DateTime::from_timestamp(7000, 0).unwrap()),
        "H" => |c| c .delete_file(IRRELEVANT_FILE)
        .set_author_date(DateTime::from_timestamp(8000, 0).unwrap()),
        "I" => |c| c.delete_file(SECOND_EXPORT_FILE)
        .set_author_date(DateTime::from_timestamp(9000, 0).unwrap()),
        "J" => |c| c.add_file(EXPORT_FILE, "add export file back")
        .set_author_date(DateTime::from_timestamp(10000, 0).unwrap()),
    };

    let dag = if opts.add_branch_commit {
        let branch_commit_changes = changes! {
            "K" => |c|  c.add_file(SECOND_EXPORT_FILE, "change export_file in a branch")
                    .set_author_date(DateTime::from_timestamp(6500, 0).unwrap())
        };
        dag_changes.extend(branch_commit_changes);

        r"
        A-B-C-D-E-F-G-H-I-J
                 \ /
                  K
        "
    } else {
        r"
        A-B-C-D-E-F-G-H-I-J
        "
    };

    let commit_id_map = create_from_dag_with_changes(ctx, &source_repo, dag, dag_changes).await?;

    let bookmark_update_ctx = bookmark(ctx, source_repo, "master");
    let _master_bookmark_key = bookmark_update_ctx.set_to(commit_id_map["J"]).await?;

    Ok((source_repo_ctx, commit_id_map))
}

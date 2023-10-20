/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;

use anyhow::anyhow;
use anyhow::Result;
use fbinit::FacebookInit;
use futures::future::try_join_all;
use maplit::hashmap;
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

const MASTER_BOOKMARK: &str = "heads/master";

// Directory and file constants.
// By convention, directories with uppercase names are exported.
const EXPORT_DIR: &str = "EXP";
const EXPORT_FILE: &str = "EXP/bar.txt";
const SECOND_EXPORT_FILE: &str = "EXP/foo.txt";

const IRRELEVANT_FILE: &str = "internal/bar.txt";
const SECOND_IRRELEVANT_FILE: &str = "internal/foo.txt";

const SECOND_EXPORT_DIR: &str = "EXP_2";
const FILE_IN_SECOND_EXPORT_DIR: &str = "EXP_2/foo.txt";

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

/// Store all relevant data about a test case to avoid harcoding and duplication
pub struct GitExportTestData {
    /// Repo created for the test case
    pub repo_ctx: RepoContext,
    /// Map of commit id/name to the actual ChangesetId
    pub commit_id_map: BTreeMap<String, ChangesetId>,
    /// ID of the HEAD commit
    pub head_id: &'static str,
    /// Paths that were used in the commits and should be known by the tests
    pub relevant_paths: HashMap<&'static str, &'static str>,
}

pub async fn build_test_repo(
    fb: FacebookInit,
    ctx: &CoreContext,
    opts: GitExportTestRepoOptions,
) -> Result<GitExportTestData> {
    let source_repo = TestRepoFactory::new(fb)?.build().await?;
    let source_repo_ctx = RepoContext::new_test(ctx.clone(), source_repo).await?;
    let source_repo = source_repo_ctx.repo();

    let relevant_paths = hashmap! {
        "export_dir" => EXPORT_DIR,
        "export_file" => EXPORT_FILE,
        "second_export_file" => SECOND_EXPORT_FILE,
        "second_export_dir" => SECOND_EXPORT_DIR,
        "file_in_second_export_dir" => FILE_IN_SECOND_EXPORT_DIR,
    };

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

    let bookmark_update_ctx = bookmark(ctx, source_repo, MASTER_BOOKMARK);
    let _master_bookmark_key = bookmark_update_ctx.set_to(commit_id_map["J"]).await?;

    Ok(GitExportTestData {
        repo_ctx: source_repo_ctx,
        commit_id_map,
        relevant_paths,
        head_id: "J",
    })
}

/// Builds a repo to test how renames of export directories are handled.
/// In this case, the directory we want to export is `bar` and it was renamed
/// from `foo` at some point throughout commit history.
pub async fn repo_with_renamed_export_path(
    fb: FacebookInit,
    ctx: &CoreContext,
) -> Result<GitExportTestData> {
    let source_repo = TestRepoFactory::new(fb)?.build().await?;
    let source_repo_ctx = RepoContext::new_test(ctx.clone(), source_repo).await?;
    let source_repo = source_repo_ctx.repo();

    const HEAD_ID: &str = "G";

    let old_export_file = "foo/file.txt";
    let new_export_file = "bar/file.txt";

    let relevant_paths = hashmap! {"old_export_dir" => "foo","new_export_dir" => "bar"};

    let mut dag_changes = changes! {
        // Create a file in the old export path
        "A" => |c| c.add_file(old_export_file, "file_to_export")
        .set_author_date(DateTime::from_timestamp(1000, 0).unwrap()),
        // Modify a random file outside of the export path, to test the
        // expected behaviour won't be affected by renames
        "B" => |c| c.add_file(IRRELEVANT_FILE, "IRRELEVANT_FILE")
        .set_author_date(DateTime::from_timestamp(2000, 0).unwrap()),
        // Change the exported file while still in the old export path.
        "C" => |c| c.add_file(old_export_file, "change EXPORT_FILE")
        .add_file(IRRELEVANT_FILE, "change IRRELEVANT_FILE")
        .set_author_date(DateTime::from_timestamp(3000, 0).unwrap()),
        // Change another irrelevant file before renaming the export path
        "D" => |c| c .add_file(IRRELEVANT_FILE, "change only IRRELEVANT_FILE")
        .set_author_date(DateTime::from_timestamp(4000, 0).unwrap()),
        // NOTE: export path is renamed in commit `E` below.
        //

        // Change a file in the export dir with the new name (i.e. the one that
        // will passed)
        "F" => |c| c.add_file(new_export_file, "change export file in new dir")
        .set_author_date(DateTime::from_timestamp(6000, 0).unwrap()),
        // Change a file in the final export path again
        HEAD_ID => |c| c.add_file(new_export_file, "change export file again")
        .set_author_date(DateTime::from_timestamp(7000, 0).unwrap()),
    };

    let other_changes = changes! {
        // Renames the export path
        "E" => |c, commits|
            c
            // Copy the export file from the old path to the new path
            .add_file_with_copy_info(
                new_export_file,
                "change EXPORT_FILE",
                (*commits.get("D").unwrap(), old_export_file)
            )
            // Delete it in the old path
            .delete_file(old_export_file)
            .set_author_date(DateTime::from_timestamp(5000, 0).unwrap()),
    };

    dag_changes.extend(other_changes);

    let commit_id_map = create_from_dag_with_changes(
        ctx,
        &source_repo,
        r##"
         A-B-C-D-E-F-G
         "##,
        dag_changes,
    )
    .await?;

    let bookmark_update_ctx = bookmark(ctx, source_repo, MASTER_BOOKMARK);
    let _master_bookmark_key = bookmark_update_ctx.set_to(commit_id_map[HEAD_ID]).await?;

    Ok(GitExportTestData {
        repo_ctx: source_repo_ctx,
        commit_id_map,
        head_id: HEAD_ID,
        relevant_paths,
    })
}

/// Scenario where multiple export paths were renamed in an order that could
/// lead to a parent in the new repo not having a file referenced in a `copy_from`
/// field.
/// In this case, the `copy_from` reference should be removed in the new commit
/// and a warning will be printed to users to show that the directory might
/// have been created in that commit by moving/copying another one.
pub async fn repo_with_multiple_renamed_export_directories(
    fb: FacebookInit,
    ctx: &CoreContext,
) -> Result<GitExportTestData> {
    let source_repo = TestRepoFactory::new(fb)?.build().await?;
    let source_repo_ctx = RepoContext::new_test(ctx.clone(), source_repo).await?;
    let source_repo = source_repo_ctx.repo();

    const HEAD_ID: &str = "D";

    let old_bar = "old_bar/file.txt";
    let new_bar = "bar/file.txt";
    let old_foo = "old_foo/file.txt";
    let new_foo = "foo/file.txt";

    let relevant_paths = hashmap! {
        "old_bar_file" => old_bar,
        "new_bar_file" => new_bar,
        "old_foo_file" => old_foo,
        "new_foo_file" => new_foo,
        "new_bar_dir" => "bar",
        "new_foo_dir" => "foo"
    };

    let mut dag_changes = changes! {
        // Create a file in the old export path
        "A" => |c| c.add_file(old_bar, "first bar")
        .set_author_date(DateTime::from_timestamp(1000, 0).unwrap()),
        // Modify a random file outside of the export path, to test the
        // expected behaviour won't be affected by renames
        "C" => |c| c.add_file(old_foo, "first foo")
        .set_author_date(DateTime::from_timestamp(3000, 0).unwrap()),
    };

    // Changesets that rename entire export directories
    let renaming_changesets = changes! {
        // Renames bar directory first
        "B" => |c, commits|
            c
            // Copy the export file from the old path to the new path
            .add_file_with_copy_info(
                new_bar,
                "first bar",
                (*commits.get("A").unwrap(), old_bar)
            )
            // Delete it in the old path
            .delete_file(old_bar)
            .set_author_date(DateTime::from_timestamp(2000, 0).unwrap()),
        // Renames foo directory right after creating it
        HEAD_ID => |c, commits|
            c
            // Copy the export file from the old path to the new path
            .add_file_with_copy_info(
                new_foo,
                "first foo",
                (*commits.get("C").unwrap(), old_foo)
            )
            // Delete it in the old path
            .delete_file(old_foo)
            .set_author_date(DateTime::from_timestamp(4000, 0).unwrap()),
    };

    dag_changes.extend(renaming_changesets);

    let commit_id_map = create_from_dag_with_changes(
        ctx,
        &source_repo,
        r##"
         A-B-C-D
         "##,
        dag_changes,
    )
    .await?;

    let bookmark_update_ctx = bookmark(ctx, source_repo, MASTER_BOOKMARK);
    let _master_bookmark_key = bookmark_update_ctx.set_to(commit_id_map[HEAD_ID]).await?;

    Ok(GitExportTestData {
        repo_ctx: source_repo_ctx,
        commit_id_map,
        head_id: HEAD_ID,
        relevant_paths,
    })
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::sparse_profile::{
    build_tree_matcher, get_profile_size, parse_sparse_profile_content, SparseProfileEntry,
};
use crate::ChangesetContext;
use crate::Mononoke;
use crate::RepoContext;
use anyhow::Result;
use context::CoreContext;
use fbinit::FacebookInit;
use fixtures::{ManyFilesDirs, TestRepoFixture};
use maplit::btreemap;
use mercurial_types::HgChangesetId;
use mononoke_types::{ChangesetId, MPath};
use pathmatcher::Matcher;
use tests_utils::{store_files, CreateCommitContext};
use types::RepoPath;

use std::collections::BTreeMap;

async fn init_sparse_profile(
    ctx: &CoreContext,
    repo: &RepoContext,
    cs_id: HgChangesetId,
) -> Result<ChangesetId> {
    let base_profile_content = r#"
        [metadata]
        title: test sparse profile
        description: For test only
        # this is a comment
        ; this is a comment as well

        [include]
        path:dir2
    "#;
    let include_test_profile_content = r#"
        %include sparse/base
        [include]
        path:dir1/subdir1
    "#;

    CreateCommitContext::new(ctx, repo.blob_repo(), vec![cs_id])
        .add_file("sparse/base", base_profile_content)
        .add_file("sparse/include", include_test_profile_content)
        .commit()
        .await
}

async fn commit_changes<T: AsRef<str>>(
    ctx: &CoreContext,
    repo: &RepoContext,
    cs_id: ChangesetId,
    changes: BTreeMap<&str, Option<T>>,
) -> Result<ChangesetId> {
    let changes = store_files(ctx, changes, repo.blob_repo()).await;
    let commit = CreateCommitContext::new(ctx, repo.blob_repo(), vec![cs_id]);
    changes
        .into_iter()
        .fold(commit, |commit, (path, change)| {
            commit.add_file_change(path, change)
        })
        .commit()
        .await
}

#[fbinit::test]
async fn sparse_profile_parsing(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), ManyFilesDirs::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists");
    let hg_cs_id = "d261bc7900818dea7c86935b3fb17a33b2e3a6b4".parse::<HgChangesetId>()?;

    let a = init_sparse_profile(&ctx, &repo, hg_cs_id).await?;

    let changeset = ChangesetContext::new(repo, a);

    let entries =
        parse_sparse_profile_content(&ctx, &changeset, &MPath::new("sparse/include")?).await?;
    assert_eq!(
        entries,
        vec![
            SparseProfileEntry::Include("path:dir2".to_string()),
            SparseProfileEntry::Include("path:dir1/subdir1".to_string())
        ]
    );

    let matcher = build_tree_matcher(entries)?;

    assert!(!matcher.matches_file(RepoPath::from_str("1")?)?);
    assert!(!matcher.matches_file(RepoPath::from_str("dir1/file1")?)?);
    assert!(matcher.matches_file(RepoPath::from_str("dir1/subdir1/file1")?)?);
    assert!(matcher.matches_file(RepoPath::from_str("dir1/subdir1/subsubdir1/file1")?)?);
    assert!(matcher.matches_file(RepoPath::from_str("dir2/file1")?)?);
    Ok(())
}

#[fbinit::test]
async fn sparse_profile_size(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), ManyFilesDirs::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists");
    let hg_cs_id = "d261bc7900818dea7c86935b3fb17a33b2e3a6b4".parse::<HgChangesetId>()?;

    let a = init_sparse_profile(&ctx, &repo, hg_cs_id).await?;
    let changeset_a = ChangesetContext::new(repo.clone(), a);
    let size = get_profile_size(&ctx, &changeset_a, &MPath::new("sparse/include")?).await?;

    assert_eq!(size, 45);

    // change size of a file which is included in sparse profile
    // profile size should change.
    let content = "1";
    let changes = btreemap! {
        "dir1/subdir1/file_1" => Some(content),
    };
    let b = commit_changes(&ctx, &repo, a, changes).await?;

    let changeset_b = ChangesetContext::new(repo.clone(), b);
    let size = get_profile_size(&ctx, &changeset_b, &MPath::new("sparse/include")?).await?;
    assert_eq!(size, 37);

    // change size of file which is NOT included in sparse profile
    // profile size should not change.
    let content = "1";
    let changes = btreemap! {
        "dir1/file_1_in_dir1" => Some(content),
    };
    let c = commit_changes(&ctx, &repo, b, changes).await?;

    let changeset_c = ChangesetContext::new(repo, c);
    let size = get_profile_size(&ctx, &changeset_c, &MPath::new("sparse/include")?).await?;
    assert_eq!(size, 37);

    Ok(())
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::sparse_profile::{build_tree_matcher, parse_sparse_profile_content, SparseProfileEntry};
use crate::ChangesetContext;
use crate::Mononoke;
use anyhow::Result;
use context::CoreContext;
use fbinit::FacebookInit;
use fixtures::many_files_dirs;
use mercurial_types::HgChangesetId;
use mononoke_types::MPath;
use pathmatcher::Matcher;
use tests_utils::CreateCommitContext;
use types::RepoPath;

#[fbinit::test]
async fn sparse_profile_parsing(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), many_files_dirs::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists");

    let hg_cs_id = "051946ed218061e925fb120dac02634f9ad40ae2".parse::<HgChangesetId>()?;

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

    let a = CreateCommitContext::new(&ctx, &repo.blob_repo(), vec![hg_cs_id])
        .add_file("sparse/base", base_profile_content)
        .add_file("sparse/include", include_test_profile_content)
        .commit()
        .await?;

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

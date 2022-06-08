/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::sparse_profile::{fetch, get_profile_delta_size, get_profile_size};
use crate::ChangesetContext;
use crate::Mononoke;
use crate::RepoContext;
use anyhow::Result;
use context::CoreContext;
use fbinit::FacebookInit;
use fixtures::{ManyFilesDirs, TestRepoFixture};
use maplit::{btreemap, hashmap};
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
    let other_profile_content = r#"
        [include]
        path:dir1
    "#;
    let top_level_files_profile_content = r#"
        [include]
        glob:{1,2}
    "#;
    let empty_profile_content = r#""#;

    CreateCommitContext::new(ctx, repo.blob_repo(), vec![cs_id])
        .add_file("sparse/base", base_profile_content)
        .add_file("sparse/include", include_test_profile_content)
        .add_file("sparse/other", other_profile_content)
        .add_file("sparse/top_level_files", top_level_files_profile_content)
        .add_file("sparse/empty", empty_profile_content)
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

    let path = "sparse/include".to_string();
    let content = fetch(path.clone(), &changeset).await?.unwrap();
    let profile = sparse::Root::from_bytes(content, path)?;
    let matcher = profile.matcher(|path| fetch(path, &changeset)).await?;

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
    let size = get_profile_size(&ctx, &changeset_a, vec![MPath::new("sparse/include")?]).await?;

    assert_eq!(size, hashmap! {"sparse/include".to_string() => 45});

    // change size of a file which is included in sparse profile
    // profile size should change.
    let content = "1";
    let changes = btreemap! {
        "dir1/subdir1/file_1" => Some(content),
    };
    let b = commit_changes(&ctx, &repo, a, changes).await?;

    let changeset_b = ChangesetContext::new(repo.clone(), b);
    let size = get_profile_size(&ctx, &changeset_b, vec![MPath::new("sparse/include")?]).await?;
    assert_eq!(size, hashmap! {"sparse/include".to_string() => 37});

    // change size of file which is NOT included in sparse profile
    // profile size should not change.
    let content = "1";
    let changes = btreemap! {
        "dir1/file_1_in_dir1" => Some(content),
    };
    let c = commit_changes(&ctx, &repo, b, changes).await?;

    let changeset_c = ChangesetContext::new(repo, c);
    let size = get_profile_size(&ctx, &changeset_c, vec![MPath::new("sparse/include")?]).await?;
    assert_eq!(size, hashmap! {"sparse/include".to_string() => 37});

    Ok(())
}

#[fbinit::test]
async fn multiple_sparse_profile_sizes(fb: FacebookInit) -> Result<()> {
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
    let profiles_map = hashmap! {
        "sparse/base".to_string() => 9,
        "sparse/include".to_string() => 45,
        "sparse/other".to_string() => 54,
        "sparse/top_level_files".to_string() => 4,
        "sparse/empty".to_string() => 423,
    };
    let profiles_names: Result<Vec<MPath>> = profiles_map.keys().map(MPath::new).collect();
    let sizes = get_profile_size(&ctx, &changeset_a, profiles_names?).await?;

    assert_eq!(sizes, profiles_map);

    Ok(())
}

#[fbinit::test]
async fn sparse_profile_delta(fb: FacebookInit) -> Result<()> {
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
    let profiles_map = hashmap! {
        "sparse/base".to_string() => 9,
        "sparse/include".to_string() => 45,
        "sparse/other".to_string() => 54,
        "sparse/top_level_files".to_string() => 4,
        "sparse/empty".to_string() => 423,
    };
    let profiles_names = profiles_map
        .keys()
        .map(MPath::new)
        .collect::<Result<Vec<_>>>()?;

    // replace the file of size 9 with the file of size 17
    let b = CreateCommitContext::new(&ctx, repo.blob_repo(), vec![a])
        .add_file("dir1/subdir1/file_2", "added new file_2\n")
        .delete_file("dir1/subdir1/file_1")
        .commit()
        .await?;
    let changeset_b = ChangesetContext::new(repo.clone(), b);
    let sizes =
        get_profile_delta_size(&ctx, &changeset_b, &changeset_a, profiles_names.clone()).await?;
    // should affect 3 profiles
    let expected = hashmap! {
        "sparse/include".to_string() => 8,
        "sparse/other".to_string() => 8,
        "sparse/empty".to_string() => 8,
    };
    assert_eq!(sizes, expected);

    // move file from one sparse profile to another
    let c = CreateCommitContext::new(&ctx, repo.blob_repo(), vec![b])
        .delete_file("dir1/subdir1/file_2")
        .add_file("dir2/file_2", "added new file_2\n")
        .add_file_with_copy_info(
            "dir2/file_3",
            "added new file_2\n",
            (b, "dir1/subdir1/file_2"),
        )
        .commit()
        .await?;
    let changeset_c = ChangesetContext::new(repo.clone(), c);
    let sizes =
        get_profile_delta_size(&ctx, &changeset_c, &changeset_b, profiles_names.clone()).await?;

    let expected = hashmap! {
        "sparse/base".to_string() => 34,
        "sparse/empty".to_string() => 17,
        "sparse/include".to_string() => 17,
        "sparse/other".to_string() => -17,
    };
    assert_eq!(sizes, expected);

    // replace directory with file
    let c1 = CreateCommitContext::new(&ctx, repo.blob_repo(), vec![c])
        .add_file("dir1/subdir1", "len4")
        .commit()
        .await?;
    let changeset_c1 = ChangesetContext::new(repo.clone(), c1);
    let sizes =
        get_profile_delta_size(&ctx, &changeset_c1, &changeset_c, profiles_names.clone()).await?;
    let expected = hashmap! {
        "sparse/other".to_string() => -23,
        "sparse/include".to_string() => -23,
        "sparse/empty".to_string() => -23,
    };
    assert_eq!(sizes, expected);

    let d = CreateCommitContext::new(&ctx, repo.blob_repo(), vec![c])
        // this essentially deletes 'dir1' directory
        .delete_file("dir1/file_1_in_dir1")
        .delete_file("dir1/file_2_in_dir1")
        .delete_file("dir1/subdir1/file_2")
        .delete_file("dir1/subdir1/subsubdir1/file_1")
        .delete_file("dir1/subdir1/subsubdir2/file_1")
        .delete_file("dir1/subdir1/subsubdir2/file_2")
        .commit()
        .await?;
    let changeset_d = ChangesetContext::new(repo.clone(), d);
    let sizes =
        get_profile_delta_size(&ctx, &changeset_d, &changeset_c, profiles_names.clone()).await?;

    let expected = hashmap! {
        "sparse/include".to_string() => -27,
        "sparse/other".to_string() => -45,
        "sparse/empty".to_string() => -45,
    };
    assert_eq!(sizes, expected);

    // change file content from 17 to 1 -> sparse should change -16
    let content = "1";
    let changes = btreemap! {
        "dir2/file_2" => Some(content),
    };
    let e = commit_changes(&ctx, &repo, d, changes).await?;

    let changeset_e = ChangesetContext::new(repo.clone(), e);
    let sizes = get_profile_delta_size(&ctx, &changeset_e, &changeset_d, profiles_names).await?;
    let expected = hashmap! {
        "sparse/base".to_string() => -16,
        "sparse/include".to_string() => -16,
        "sparse/empty".to_string() => -16,
    };
    assert_eq!(sizes, expected);

    Ok(())
}

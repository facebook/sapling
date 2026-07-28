/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Unit tests for `multi_repo_land_lib`.

use std::sync::Arc;

use anyhow::Result;
use blobstore::Loadable;
use bytes::Bytes;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_macros::mononoke;
use mononoke_repos::MononokeRepos;
use mononoke_types::NonRootMPath;
use multi_repo_land_lib::RepoProvider;
use multi_repo_land_lib::create_manifest_commit;
use repo_blobstore::RepoBlobstoreRef;
use tests_utils::BasicTestRepo;
use tests_utils::CreateCommitContext;

#[mononoke::fbinit_test]
async fn test_creates_commit_on_top_of_parent(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BasicTestRepo = test_repo_factory::build_empty(fb).await?;

    let parent = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("existing", "data")
        .commit()
        .await?;

    let manifest_path = NonRootMPath::new("manifest.xml")?;
    let cs_id = create_manifest_commit(
        &ctx,
        &repo,
        parent,
        &manifest_path,
        Bytes::from("<manifest/>"),
        "test_service",
    )
    .await?;

    // The new commit should have the parent as its only parent.
    let bcs = cs_id.load(&ctx, repo.repo_blobstore()).await?;
    assert_eq!(bcs.parents().collect::<Vec<_>>(), vec![parent]);
    assert_ne!(cs_id, parent);
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_commit_has_correct_file_change(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BasicTestRepo = test_repo_factory::build_empty(fb).await?;

    let parent = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("readme", "hello")
        .commit()
        .await?;

    let manifest_path = NonRootMPath::new("path/to/manifest.xml")?;
    let content = b"<manifest><project/></manifest>";
    let cs_id = create_manifest_commit(
        &ctx,
        &repo,
        parent,
        &manifest_path,
        Bytes::from(content.as_slice()),
        "test_service",
    )
    .await?;

    let bcs = cs_id.load(&ctx, repo.repo_blobstore()).await?;

    // Verify the manifest file is the only change.
    let changed_paths: Vec<_> = bcs.file_changes().map(|(p, _)| p.clone()).collect();
    assert_eq!(changed_paths, vec![manifest_path]);

    // Verify author and message.
    assert_eq!(bcs.author(), "test_service");
    assert!(bcs.message().contains("manifest.xml"));

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_commit_stores_correct_content(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BasicTestRepo = test_repo_factory::build_empty(fb).await?;

    let parent = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("readme", "hello")
        .commit()
        .await?;

    let manifest_path = NonRootMPath::new("manifest.xml")?;
    let content = Bytes::from("test content 12345");
    let cs_id =
        create_manifest_commit(&ctx, &repo, parent, &manifest_path, content.clone(), "svc").await?;

    // Verify the stored file size matches.
    let bcs = cs_id.load(&ctx, repo.repo_blobstore()).await?;
    let (_, file_change) = bcs.file_changes().next().unwrap();
    let tracked = file_change.simplify().unwrap();
    assert_eq!(tracked.size(), content.len() as u64);

    Ok(())
}

/// The blanket `RepoProvider` impl over `MononokeRepos` returns a repo by name
/// when present and `None` otherwise.
#[mononoke::test]
fn test_repo_provider_blanket_impl() {
    struct DummyRepo;

    let repos: MononokeRepos<DummyRepo> = MononokeRepos::new();
    repos.add("present", 0, DummyRepo);

    let provider: &dyn RepoProvider<DummyRepo> = &repos;
    assert!(
        provider.get_by_name("present").is_some(),
        "known repo should resolve"
    );
    assert!(
        provider.get_by_name("absent").is_none(),
        "unknown repo should be None"
    );
    // Confirm the returned handle is an Arc to the stored repo.
    let _handle: Arc<DummyRepo> = provider.get_by_name("present").expect("present exists");
}

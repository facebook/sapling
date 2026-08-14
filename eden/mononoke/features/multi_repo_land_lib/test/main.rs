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
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_git_mapping::BonsaiGitMappingEntry;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkUpdateReason;
use bookmarks::Bookmarks;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use bytes::Bytes;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use dbbookmarks::store::SqlBookmarks;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use metaconfig_types::RepoConfig;
use mononoke_macros::mononoke;
use mononoke_repos::MononokeRepos;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use mononoke_types::hash::GitSha1;
use multi_repo_land_lib::CasBaseline;
use multi_repo_land_lib::ManifestCommitSpec;
use multi_repo_land_lib::RepinOptions;
use multi_repo_land_lib::RepinOutcome;
use multi_repo_land_lib::RepoProvider;
use multi_repo_land_lib::ResolveEntry;
use multi_repo_land_lib::ResolveOutcome;
use multi_repo_land_lib::create_manifest_commit;
use multi_repo_land_lib::log_scribe_bookmark_update;
use multi_repo_land_lib::prepare_manifest_commit;
use multi_repo_land_lib::repin_manifest_branch;
use multi_repo_land_lib::resolve_bookmarks_cross_repo;
use phases::Phases;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use repo_update_logger::BookmarkInfo;
use repo_update_logger::BookmarkOperation;
use test_repo_factory::TestRepoFactory;
use tests_utils::BasicTestRepo;
use tests_utils::CreateCommitContext;
use tests_utils::bookmark;

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

/// Test repo carrying every facet the lib functions under test need:
/// `resolve_bookmarks_cross_repo` (`RepoIdentityRef` + `SqlBookmarksRef`),
/// `prepare_manifest_commit` (blobstore/filestore/commit-graph/derived-data),
/// and `log_scribe_bookmark_update` (repo-config/git-mapping/globalrev/phases),
/// plus the facets the `tests_utils` commit/bookmark helpers require.
/// `BasicTestRepo` is unusable here because it lacks the concrete `SqlBookmarks`
/// facet.
#[facet::container]
#[derive(Clone)]
struct TestRepo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_config: RepoConfig,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    sql_bookmarks: SqlBookmarks,

    #[facet]
    filestore_config: FilestoreConfig,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    phases: dyn Phases,
}

/// A mixed batch exercises every outcome (unknown repo, invalid/missing
/// bookmark, missing git mapping, success across two repos) and asserts the
/// results line up with the input order.
///
/// Both repos are built from a single `TestRepoFactory` so they share one
/// in-memory metadata DB — mirroring the single-shard assumption the resolve
/// relies on for its cross-repo `IN (...)` queries.
///
/// NOTE: the `BATCH_SIZE = 500` chunk boundary is not exercised here —
/// provisioning 500+ repos in a unit test is impractical. The batching is a
/// straightforward `.chunks(500)` loop over the deduped pairs.
#[mononoke::fbinit_test]
async fn test_resolve_bookmarks_cross_repo_mixed_batch(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let mut factory = TestRepoFactory::new(fb)?;
    let repo_a: TestRepo = factory
        .with_id(RepositoryId::new(0))
        .with_name("repo_a")
        .build()
        .await?;
    let repo_b: TestRepo = factory
        .with_id(RepositoryId::new(1))
        .with_name("repo_b")
        .build()
        .await?;

    // repo_a: "main" -> commit with a git mapping (success).
    let main_a = CreateCommitContext::new_root(&ctx, &repo_a)
        .add_file("f", "a-main")
        .commit()
        .await?;
    bookmark(&ctx, &repo_a, "main")
        .create_publishing(main_a)
        .await?;
    let sha_a = GitSha1::from_byte_array([0xAA; 20]);
    repo_a
        .bonsai_git_mapping()
        .add(&ctx, BonsaiGitMappingEntry::new(sha_a, main_a))
        .await?;

    // repo_a: "nomapping" -> commit WITHOUT a git mapping.
    let nomap_a = CreateCommitContext::new_root(&ctx, &repo_a)
        .add_file("f", "a-nomap")
        .commit()
        .await?;
    bookmark(&ctx, &repo_a, "nomapping")
        .create_publishing(nomap_a)
        .await?;

    // repo_b: "release" -> commit with a git mapping (success, second repo).
    let main_b = CreateCommitContext::new_root(&ctx, &repo_b)
        .add_file("f", "b-release")
        .commit()
        .await?;
    bookmark(&ctx, &repo_b, "release")
        .create_publishing(main_b)
        .await?;
    let sha_b = GitSha1::from_byte_array([0xBB; 20]);
    repo_b
        .bonsai_git_mapping()
        .add(&ctx, BonsaiGitMappingEntry::new(sha_b, main_b))
        .await?;

    let repos: MononokeRepos<TestRepo> = MononokeRepos::new();
    repos.add("repo_a", 0, repo_a);
    repos.add("repo_b", 1, repo_b);

    let entries = vec![
        // 0: success (first repo -> supplies the shared read connection).
        ResolveEntry {
            repo_name: "repo_a".to_string(),
            bookmark_name: "main".to_string(),
        },
        // 1: unknown repo.
        ResolveEntry {
            repo_name: "ghost".to_string(),
            bookmark_name: "main".to_string(),
        },
        // 2: invalid (non-ascii) bookmark name.
        ResolveEntry {
            repo_name: "repo_a".to_string(),
            bookmark_name: "inval\u{00e9}d".to_string(),
        },
        // 3: valid name, but no such bookmark.
        ResolveEntry {
            repo_name: "repo_a".to_string(),
            bookmark_name: "missing".to_string(),
        },
        // 4: bookmark exists, but no git mapping.
        ResolveEntry {
            repo_name: "repo_a".to_string(),
            bookmark_name: "nomapping".to_string(),
        },
        // 5: success in the second repo.
        ResolveEntry {
            repo_name: "repo_b".to_string(),
            bookmark_name: "release".to_string(),
        },
    ];

    let results = resolve_bookmarks_cross_repo(&ctx, &repos, &entries).await?;

    // One result per input, in the same order.
    assert_eq!(results.len(), entries.len(), "one result per input entry");
    for (entry, result) in entries.iter().zip(results.iter()) {
        assert_eq!(
            (result.repo_name.as_str(), result.bookmark_name.as_str()),
            (entry.repo_name.as_str(), entry.bookmark_name.as_str()),
            "results must stay aligned with input order",
        );
    }

    assert_eq!(results[0].outcome, ResolveOutcome::Resolved(sha_a));
    assert_eq!(
        results[1].outcome,
        ResolveOutcome::Error("unknown repo: ghost".to_string()),
    );
    assert_eq!(
        results[2].outcome,
        ResolveOutcome::Error("invalid bookmark name: inval\u{00e9}d".to_string()),
    );
    assert_eq!(
        results[3].outcome,
        ResolveOutcome::Error("bookmark not found".to_string()),
    );
    assert_eq!(
        results[4].outcome,
        ResolveOutcome::Error("git mapping not found".to_string()),
    );
    assert_eq!(results[5].outcome, ResolveOutcome::Resolved(sha_b));

    Ok(())
}

/// An empty request returns an empty result set without touching any repo.
#[mononoke::fbinit_test]
async fn test_resolve_bookmarks_cross_repo_empty(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repos: MononokeRepos<TestRepo> = MononokeRepos::new();

    let results = resolve_bookmarks_cross_repo(&ctx, &repos, &[]).await?;
    assert!(results.is_empty(), "empty input yields empty output");

    Ok(())
}

/// With no parent override, the generated commit is built on the bookmark head,
/// `old_cs` is that head (the CAS baseline), and the manifest file is the only
/// change. A `MappedGitCommitId` is pre-derived.
#[mononoke::fbinit_test]
async fn test_prepare_manifest_commit_default_parent(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let head = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "v1")
        .commit()
        .await?;
    let bm = bookmark(&ctx, &repo, "manifest")
        .create_publishing(head)
        .await?;

    let manifest_path = NonRootMPath::new("default.xml")?;
    let prepared = prepare_manifest_commit(
        &ctx,
        &repo,
        ManifestCommitSpec {
            bookmark: &bm,
            manifest_path: &manifest_path,
            content: Bytes::from("<manifest/>"),
            service_identity: "svc",
            parent_override: None,
            baseline: CasBaseline::CurrentHead,
        },
    )
    .await?;

    assert_eq!(
        prepared.old_cs, head,
        "old_cs must be the live head (CAS baseline)"
    );
    assert_ne!(prepared.new_cs, head, "a new commit must be created");

    let bcs = prepared.new_cs.load(&ctx, repo.repo_blobstore()).await?;
    assert_eq!(
        bcs.parents().collect::<Vec<_>>(),
        vec![head],
        "default parent is the bookmark head",
    );
    let changed: Vec<_> = bcs.file_changes().map(|(p, _)| p.clone()).collect();
    assert_eq!(
        changed,
        vec![manifest_path],
        "the manifest file is the only change",
    );

    // A SHA1 git commit id (20 bytes) was pre-derived.
    assert_eq!(
        prepared.mapped_git.oid().as_ref().len(),
        20,
        "mapped git commit id should be a 20-byte SHA1",
    );

    Ok(())
}

/// A parent override (the user's manifest commit) becomes the generated commit's
/// parent, while `old_cs` still reflects the live bookmark head.
#[mononoke::fbinit_test]
async fn test_prepare_manifest_commit_parent_override(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let head = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "v1")
        .commit()
        .await?;
    let bm = bookmark(&ctx, &repo, "manifest")
        .create_publishing(head)
        .await?;

    // The user's own manifest commit, one commit ahead of the head.
    let user_commit = CreateCommitContext::new(&ctx, &repo, vec![head])
        .add_file("user", "change")
        .commit()
        .await?;

    let manifest_path = NonRootMPath::new("override.xml")?;
    let prepared = prepare_manifest_commit(
        &ctx,
        &repo,
        ManifestCommitSpec {
            bookmark: &bm,
            manifest_path: &manifest_path,
            content: Bytes::from("<manifest/>"),
            service_identity: "svc",
            parent_override: Some(user_commit),
            baseline: CasBaseline::CurrentHead,
        },
    )
    .await?;

    assert_eq!(
        prepared.old_cs, head,
        "old_cs stays the head even with an override parent",
    );
    let bcs = prepared.new_cs.load(&ctx, repo.repo_blobstore()).await?;
    assert_eq!(
        bcs.parents().collect::<Vec<_>>(),
        vec![user_commit],
        "commit is built on the override parent",
    );

    Ok(())
}

/// A missing manifest bookmark yields the exact "manifest bookmark not found"
/// error, preserving the service's behavior.
#[mononoke::fbinit_test]
async fn test_prepare_manifest_commit_bookmark_not_found(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let bm = BookmarkKey::new("absent")?;
    let manifest_path = NonRootMPath::new("m.xml")?;
    let err = prepare_manifest_commit(
        &ctx,
        &repo,
        ManifestCommitSpec {
            bookmark: &bm,
            manifest_path: &manifest_path,
            content: Bytes::from("<manifest/>"),
            service_identity: "svc",
            parent_override: None,
            baseline: CasBaseline::CurrentHead,
        },
    )
    .await
    .expect_err("absent bookmark must error");

    assert!(
        err.to_string().contains("manifest bookmark not found"),
        "error should name the missing bookmark, got: {err}",
    );

    Ok(())
}

/// `log_scribe_bookmark_update` is fire-and-forget: on a repo with no
/// `bookmark_logging_destination` (the default test config) it runs the
/// draft-ancestor walk and the gated no-op logging to completion without
/// panicking, and moves no bookmark.
#[mononoke::fbinit_test]
async fn test_log_scribe_bookmark_update_no_destination_is_noop(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let head = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "v1")
        .commit()
        .await?;
    let bm = bookmark(&ctx, &repo, "main")
        .create_publishing(head)
        .await?;
    // A draft child of the (public) head, standing in for the moved-to commit.
    let new_target = CreateCommitContext::new(&ctx, &repo, vec![head])
        .add_file("next", "v2")
        .commit()
        .await?;

    let info = BookmarkInfo {
        bookmark_name: bm.clone(),
        bookmark_kind: BookmarkKind::Publishing,
        operation: BookmarkOperation::Update(head, new_target),
        reason: BookmarkUpdateReason::MultiRepoLand,
    };

    // Returns () without panicking despite there being no logging destination.
    log_scribe_bookmark_update(&ctx, &repo, &info, Some(new_target)).await;

    // Logging is read-only: the bookmark must be untouched.
    assert_eq!(
        repo.bookmarks()
            .get(ctx.clone(), &bm, Freshness::MostRecent)
            .await?,
        Some(head),
        "scribe logging must not move the bookmark",
    );

    Ok(())
}

/// `repin_manifest_branch` generates the manifest commit on the current head
/// and atomically moves the bookmark to it, returning `Moved` with the new
/// changeset and its pre-derived git identity.
#[mononoke::fbinit_test]
async fn test_repin_manifest_branch_moves_bookmark(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let head = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "v1")
        .commit()
        .await?;
    let bm = bookmark(&ctx, &repo, "branch")
        .create_publishing(head)
        .await?;

    let manifest_path = NonRootMPath::new("static.xml")?;
    let outcome = repin_manifest_branch(
        &ctx,
        &repo,
        &bm,
        &manifest_path,
        Bytes::from("<manifest/>"),
        "svc",
        &RepinOptions::default(),
    )
    .await?;

    let new_cs = match outcome {
        RepinOutcome::Moved { new_cs, mapped_git } => {
            assert_eq!(
                mapped_git.oid().as_ref().len(),
                20,
                "a 20-byte git sha1 should be pre-derived",
            );
            new_cs
        }
        RepinOutcome::CasFailure => panic!("expected Moved, got CasFailure"),
    };
    assert_ne!(new_cs, head, "a new commit must be created");

    // The branch head now points at the generated static.xml commit.
    assert_eq!(
        repo.bookmarks()
            .get(ctx.clone(), &bm, Freshness::MostRecent)
            .await?,
        Some(new_cs),
        "the branch head must move to new_cs",
    );

    // That commit is built on the old head and changes only the manifest file.
    let bcs = new_cs.load(&ctx, repo.repo_blobstore()).await?;
    assert_eq!(bcs.parents().collect::<Vec<_>>(), vec![head]);
    let changed: Vec<_> = bcs.file_changes().map(|(p, _)| p.clone()).collect();
    assert_eq!(changed, vec![manifest_path]);

    Ok(())
}

/// When the CAS baseline is stale, the transaction fails and `repin_manifest_branch`
/// returns `CasFailure` without moving the bookmark. Modeled deterministically
/// with a scratch bookmark: `prepare_manifest_commit` reads its head (kind-
/// agnostic), but the transaction's CAS only matches PUBLISHING rows, so it
/// matches zero rows — the same `!is_success()` a concurrent publishing move
/// produces.
#[mononoke::fbinit_test]
async fn test_repin_manifest_branch_cas_failure(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let head = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "v1")
        .commit()
        .await?;
    let bm = bookmark(&ctx, &repo, "branch").create_scratch(head).await?;

    let manifest_path = NonRootMPath::new("static.xml")?;
    let outcome = repin_manifest_branch(
        &ctx,
        &repo,
        &bm,
        &manifest_path,
        Bytes::from("<manifest/>"),
        "svc",
        &RepinOptions::default(),
    )
    .await?;

    assert!(
        matches!(outcome, RepinOutcome::CasFailure),
        "a stale CAS baseline must yield CasFailure, got {outcome:?}",
    );

    // The bookmark is left untouched.
    assert_eq!(
        repo.bookmarks()
            .get(ctx.clone(), &bm, Freshness::MostRecent)
            .await?,
        Some(head),
        "a CAS failure must leave the bookmark unchanged",
    );

    Ok(())
}

/// With `log_scribe = false` the bookmark still moves; only the fire-and-forget
/// scribe emission is skipped.
#[mononoke::fbinit_test]
async fn test_repin_manifest_branch_no_scribe_still_moves(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

    let head = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "v1")
        .commit()
        .await?;
    let bm = bookmark(&ctx, &repo, "branch")
        .create_publishing(head)
        .await?;

    let manifest_path = NonRootMPath::new("static.xml")?;
    let outcome = repin_manifest_branch(
        &ctx,
        &repo,
        &bm,
        &manifest_path,
        Bytes::from("<manifest/>"),
        "svc",
        &RepinOptions {
            log_scribe: false,
            ..RepinOptions::default()
        },
    )
    .await?;

    let new_cs = match outcome {
        RepinOutcome::Moved { new_cs, .. } => new_cs,
        RepinOutcome::CasFailure => panic!("expected Moved, got CasFailure"),
    };
    assert_eq!(
        repo.bookmarks()
            .get(ctx.clone(), &bm, Freshness::MostRecent)
            .await?,
        Some(new_cs),
        "the branch head must move even with scribe disabled",
    );

    Ok(())
}

/// The whole point of `CasBaseline::GeneratedFrom`: a caller that generated its
/// content from one head must CAS against THAT head. Re-reading would adopt
/// whatever landed in between as the baseline, so the CAS would succeed and
/// silently overwrite it.
#[mononoke::fbinit_test]
async fn generated_from_pins_the_cas_baseline_against_a_concurrent_land(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BasicTestRepo = test_repo_factory::build_empty(fb).await?;

    let generated_from = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "v1")
        .commit()
        .await?;
    let bm = bookmark(&ctx, &repo, "manifest")
        .create_publishing(generated_from)
        .await?;

    // Someone else lands between our read and our write.
    let concurrent = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base", "v2")
        .commit()
        .await?;
    bookmark(&ctx, &repo, "manifest").set_to(concurrent).await?;

    let manifest_path = NonRootMPath::new("default.xml")?;

    let stale = prepare_manifest_commit(
        &ctx,
        &repo,
        ManifestCommitSpec {
            bookmark: &bm,
            manifest_path: &manifest_path,
            content: Bytes::from("<manifest/>"),
            service_identity: "svc",
            parent_override: None,
            baseline: CasBaseline::GeneratedFrom(generated_from),
        },
    )
    .await?;
    assert_eq!(
        stale.old_cs, generated_from,
        "the baseline must be the head we generated from, so the CAS fails"
    );

    let adopting = prepare_manifest_commit(
        &ctx,
        &repo,
        ManifestCommitSpec {
            bookmark: &bm,
            manifest_path: &manifest_path,
            content: Bytes::from("<manifest/>"),
            service_identity: "svc",
            parent_override: None,
            baseline: CasBaseline::CurrentHead,
        },
    )
    .await?;
    assert_eq!(
        adopting.old_cs, concurrent,
        "without it the concurrent land is adopted as the baseline"
    );

    Ok(())
}

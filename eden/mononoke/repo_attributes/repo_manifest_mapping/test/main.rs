/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_macros::mononoke;
use mononoke_types::RepositoryId;
use repo_manifest_mapping::ManifestBranch;
use repo_manifest_mapping::MembershipEdge;
use repo_manifest_mapping::NoopRepoManifestMapping;
use repo_manifest_mapping::RepoBranch;
use repo_manifest_mapping::RepoManifestMapping;
use repo_manifest_mapping::RepoName;
use repo_manifest_mapping::SqlRepoManifestMapping;
use repo_manifest_mapping::SqlRepoManifestMappingBuilder;
use repo_manifest_mapping::Staleness;
use repo_manifest_mapping::TestRepoManifestMapping;
use sql_construct::SqlConstruct;

fn mb(s: &str) -> ManifestBranch {
    ManifestBranch(s.to_string())
}

fn rn(s: &str) -> RepoName {
    RepoName(s.to_string())
}

fn rb(s: &str) -> RepoBranch {
    RepoBranch(s.to_string())
}

fn rid(id: i32) -> RepositoryId {
    RepositoryId::new(id)
}

fn edge(repo: &str, branch: &str) -> MembershipEdge {
    MembershipEdge::new(rn(repo), rb(branch))
}

fn new_store() -> Result<SqlRepoManifestMapping> {
    Ok(SqlRepoManifestMappingBuilder::with_sqlite_in_memory()?.build())
}

// 1. insert edges -> reverse read returns the right (manifest_repo_id,
//    manifest_branch) set.
#[mononoke::fbinit_test]
async fn test_insert_and_reverse_lookup(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = new_store()?;
    let aosp = rid(1);

    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("main"),
            &[
                edge("platform/build", "aosp-main"),
                edge("platform/art", "aosp-main"),
            ],
            None,
        )
        .await?;

    // Hot fan-out read: callers tolerate replica staleness.
    let branches = store
        .manifest_branches_for_repo(
            &ctx,
            &rn("platform/build"),
            &rb("aosp-main"),
            Staleness::MaybeStale,
        )
        .await?;
    assert_eq!(
        branches,
        vec![(aosp, mb("main"))],
        "platform/build@aosp-main belongs to aosp/main"
    );

    Ok(())
}

// 2. forward read is scoped by (manifest_repo_id, manifest_branch).
#[mononoke::fbinit_test]
async fn test_members_for_manifest_branch(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = new_store()?;
    let aosp = rid(1);

    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("main"),
            &[
                edge("platform/build", "aosp-main"),
                edge("platform/art", "aosp-dev"),
            ],
            None,
        )
        .await?;

    let members = store
        .members_for_manifest_branch(&ctx, aosp, &mb("main"), Staleness::MostRecent)
        .await?;
    assert_eq!(
        members,
        vec![
            // The store's `ORDER BY repo_name, repo_branch` makes this a
            // contract: "platform/art" < "platform/build".
            edge("platform/art", "aosp-dev"),
            edge("platform/build", "aosp-main"),
        ]
    );

    // Scoped: an unknown manifest repo id has no members for the same branch.
    let empty = store
        .members_for_manifest_branch(&ctx, rid(999), &mb("main"), Staleness::MostRecent)
        .await?;
    assert!(empty.is_empty(), "unknown manifest repo has no members");

    Ok(())
}

// 3. The reverse read is idempotent across repeated replaces of the same edge.
//    (The SELECT DISTINCT is defensive only: the four-column UNIQUE key already
//    precludes duplicate rows, so a fixed (repo_name, repo_branch) can never map
//    a given (manifest_repo_id, manifest_branch) to more than one row.)
#[mononoke::fbinit_test]
async fn test_reverse_read_idempotent_replace(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = new_store()?;
    let aosp = rid(1);

    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("main"),
            &[edge("platform/build", "aosp-main")],
            None,
        )
        .await?;
    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("main"),
            &[edge("platform/build", "aosp-main")],
            None,
        )
        .await?;

    let branches = store
        .manifest_branches_for_repo(
            &ctx,
            &rn("platform/build"),
            &rb("aosp-main"),
            Staleness::MaybeStale,
        )
        .await?;
    assert_eq!(
        branches,
        vec![(aosp, mb("main"))],
        "reverse read must return the manifest branch exactly once"
    );

    Ok(())
}

// 4. NEGATIVE: wrong repo_branch -> empty (compound-key discrimination).
#[mononoke::fbinit_test]
async fn test_negative_wrong_repo_branch(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = new_store()?;
    let aosp = rid(1);

    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("main"),
            &[edge("platform/build", "aosp-main")],
            None,
        )
        .await?;

    let branches = store
        .manifest_branches_for_repo(
            &ctx,
            &rn("platform/build"),
            &rb("aosp-dev"),
            Staleness::MaybeStale,
        )
        .await?;
    assert!(
        branches.is_empty(),
        "the right repo on the wrong branch must not match"
    );

    // Sanity: the correct compound key does match.
    let branches = store
        .manifest_branches_for_repo(
            &ctx,
            &rn("platform/build"),
            &rb("aosp-main"),
            Staleness::MaybeStale,
        )
        .await?;
    assert_eq!(branches, vec![(aosp, mb("main"))]);

    Ok(())
}

// 5. replace_membership is idempotent: applying the same set twice yields the
//    same rows.
#[mononoke::fbinit_test]
async fn test_replace_membership_idempotent(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = new_store()?;
    let aosp = rid(1);

    let edges = [
        edge("platform/build", "aosp-main"),
        edge("platform/art", "aosp-main"),
    ];

    store
        .replace_membership(&ctx, aosp, &mb("main"), &edges, None)
        .await?;
    let first = store
        .members_for_manifest_branch(&ctx, aosp, &mb("main"), Staleness::MostRecent)
        .await?;

    store
        .replace_membership(&ctx, aosp, &mb("main"), &edges, None)
        .await?;
    let second = store
        .members_for_manifest_branch(&ctx, aosp, &mb("main"), Staleness::MostRecent)
        .await?;

    assert_eq!(
        first, second,
        "applying the same membership set twice must yield identical rows"
    );
    assert_eq!(second.len(), 2);

    Ok(())
}

// 6. replace_membership actually REPLACES: set A then set B -> only B remains.
#[mononoke::fbinit_test]
async fn test_replace_membership_replaces(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = new_store()?;
    let aosp = rid(1);

    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("main"),
            &[
                edge("platform/build", "aosp-main"),
                edge("platform/art", "aosp-main"),
            ],
            None,
        )
        .await?;
    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("main"),
            &[edge("platform/frameworks", "aosp-main")],
            None,
        )
        .await?;

    // Read-your-writes: assert we observe the just-committed replacement.
    let members = store
        .members_for_manifest_branch(&ctx, aosp, &mb("main"), Staleness::MostRecent)
        .await?;
    assert_eq!(
        members,
        vec![edge("platform/frameworks", "aosp-main")],
        "only set B should remain after the second replace"
    );

    // Reverse: members of set A no longer resolve to aosp/main; set B does.
    assert!(
        store
            .manifest_branches_for_repo(
                &ctx,
                &rn("platform/build"),
                &rb("aosp-main"),
                Staleness::MostRecent,
            )
            .await?
            .is_empty(),
        "set A member must be gone from the reverse index"
    );
    assert_eq!(
        store
            .manifest_branches_for_repo(
                &ctx,
                &rn("platform/frameworks"),
                &rb("aosp-main"),
                Staleness::MostRecent,
            )
            .await?,
        vec![(aosp, mb("main"))]
    );

    Ok(())
}

// 7. CASE-SENSITIVITY: "Foo" and "foo" are distinct keys (guards the binary
//    collation intent), on both the manifest-branch and repo-name sides.
#[mononoke::fbinit_test]
async fn test_case_sensitivity(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = new_store()?;
    let aosp = rid(1);

    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("Foo"),
            &[edge("platform/build", "aosp-main")],
            None,
        )
        .await?;
    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("foo"),
            &[edge("platform/art", "aosp-main")],
            None,
        )
        .await?;

    assert_eq!(
        store
            .members_for_manifest_branch(&ctx, aosp, &mb("Foo"), Staleness::MostRecent)
            .await?,
        vec![edge("platform/build", "aosp-main")],
        "manifest branch 'Foo' is distinct from 'foo'"
    );
    assert_eq!(
        store
            .members_for_manifest_branch(&ctx, aosp, &mb("foo"), Staleness::MostRecent)
            .await?,
        vec![edge("platform/art", "aosp-main")]
    );

    // Distinct repo names "Platform" vs "platform" under one manifest branch ->
    // two separate member rows.
    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("main"),
            &[edge("Platform", "aosp-main"), edge("platform", "aosp-main")],
            None,
        )
        .await?;
    assert_eq!(
        store
            .members_for_manifest_branch(&ctx, aosp, &mb("main"), Staleness::MostRecent)
            .await?
            .len(),
        2,
        "'Platform' and 'platform' are distinct member rows"
    );
    assert_eq!(
        store
            .manifest_branches_for_repo(
                &ctx,
                &rn("Platform"),
                &rb("aosp-main"),
                Staleness::MostRecent,
            )
            .await?,
        vec![(aosp, mb("main"))]
    );

    Ok(())
}

// 8. Two DIFFERENT manifest_repo_ids sharing the same manifest_branch name are
//    distinct rows: forward reads are scoped, and replacing one leaves the
//    other intact.
#[mononoke::fbinit_test]
async fn test_manifest_repo_id_scoping(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = new_store()?;
    let aosp = rid(1);
    let zephyr = rid(2);

    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("main"),
            &[edge("platform/build", "aosp-main")],
            None,
        )
        .await?;
    store
        .replace_membership(
            &ctx,
            zephyr,
            &mb("main"),
            &[edge("zephyr/hal", "main")],
            None,
        )
        .await?;

    assert_eq!(
        store
            .members_for_manifest_branch(&ctx, aosp, &mb("main"), Staleness::MostRecent)
            .await?,
        vec![edge("platform/build", "aosp-main")],
        "aosp/main and zephyr/main are distinct despite the shared branch name"
    );
    assert_eq!(
        store
            .members_for_manifest_branch(&ctx, zephyr, &mb("main"), Staleness::MostRecent)
            .await?,
        vec![edge("zephyr/hal", "main")]
    );

    // Replacing aosp/main must not touch zephyr/main.
    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("main"),
            &[edge("platform/art", "aosp-main")],
            None,
        )
        .await?;
    assert_eq!(
        store
            .members_for_manifest_branch(&ctx, aosp, &mb("main"), Staleness::MostRecent)
            .await?,
        vec![edge("platform/art", "aosp-main")]
    );
    assert_eq!(
        store
            .members_for_manifest_branch(&ctx, zephyr, &mb("main"), Staleness::MostRecent)
            .await?,
        vec![edge("zephyr/hal", "main")],
        "zephyr/main is untouched by an aosp/main replace"
    );

    Ok(())
}

// 9. The reverse read fans out across manifest repos: a member repo that
//    belongs to branches in two different manifest repos returns both.
#[mononoke::fbinit_test]
async fn test_reverse_read_fans_out_across_manifest_repos(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = new_store()?;
    let aosp = rid(1);
    let zephyr = rid(2);

    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("main"),
            &[edge("shared/common", "stable")],
            None,
        )
        .await?;
    store
        .replace_membership(
            &ctx,
            zephyr,
            &mb("release"),
            &[edge("shared/common", "stable")],
            None,
        )
        .await?;

    let branches = store
        .manifest_branches_for_repo(
            &ctx,
            &rn("shared/common"),
            &rb("stable"),
            Staleness::MaybeStale,
        )
        .await?;
    // No local sort: `ORDER BY manifest_repo_id` makes aosp (id 1) precede
    // zephyr (id 2) contractually.
    assert_eq!(
        branches,
        vec![(aosp, mb("main")), (zephyr, mb("release"))],
        "shared/common@stable is a member of both aosp/main and zephyr/release"
    );

    Ok(())
}

// 10. watermark get (absent -> None) / set / get keyed by repo_id, and
//     replace_membership with a watermark advances it in the same txn.
#[mononoke::fbinit_test]
async fn test_watermark(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = new_store()?;
    let aosp = rid(1);
    let zephyr = rid(2);

    assert_eq!(
        store
            .get_watermark(&ctx, aosp, Staleness::MaybeStale)
            .await?,
        None,
        "absent watermark reads as None"
    );

    store.set_watermark(&ctx, aosp, 42).await?;
    assert_eq!(
        store
            .get_watermark(&ctx, aosp, Staleness::MostRecent)
            .await?,
        Some(42)
    );

    store.set_watermark(&ctx, aosp, 100).await?;
    assert_eq!(
        store
            .get_watermark(&ctx, aosp, Staleness::MostRecent)
            .await?,
        Some(100),
        "watermark updates"
    );

    // Keyed by repo_id: a different manifest repo's watermark is independent.
    assert_eq!(
        store
            .get_watermark(&ctx, zephyr, Staleness::MaybeStale)
            .await?,
        None,
        "watermark is keyed per manifest repo"
    );

    // Advancing the watermark inside a replace transaction: both the edges and
    // the new watermark must be visible after commit.
    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("main"),
            &[edge("platform/build", "aosp-main")],
            Some(200),
        )
        .await?;
    assert_eq!(
        store
            .get_watermark(&ctx, aosp, Staleness::MostRecent)
            .await?,
        Some(200),
        "watermark advanced in the replace transaction"
    );
    assert_eq!(
        store
            .manifest_branches_for_repo(
                &ctx,
                &rn("platform/build"),
                &rb("aosp-main"),
                Staleness::MostRecent,
            )
            .await?,
        vec![(aosp, mb("main"))],
        "edges from the same transaction are visible"
    );

    Ok(())
}

// 11. replace_membership de-duplicates the input batch. A real manifest can list
//     the same (repo_name, repo_branch) more than once (e.g. the same repo pinned
//     at the same branch via two project paths — the path is not part of the
//     edge), so duplicates must collapse to a single row rather than fail. The
//     watermark supplied alongside advances exactly once, and prior state is
//     cleanly replaced.
#[mononoke::fbinit_test]
async fn test_replace_membership_dedups_duplicate_edges(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = new_store()?;
    let aosp = rid(1);

    // Commit prior state for aosp/main (edges + watermark).
    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("main"),
            &[
                edge("platform/build", "aosp-main"),
                edge("platform/art", "aosp-main"),
            ],
            Some(5),
        )
        .await?;

    // A replace whose batch contains a duplicated edge must SUCCEED and collapse
    // the duplicate to one row (membership is a set), advancing the watermark once
    // and fully replacing the prior state.
    let dup = edge("platform/frameworks", "aosp-main");
    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("main"),
            &[dup.clone(), dup.clone()],
            Some(9),
        )
        .await?;

    let members = store
        .members_for_manifest_branch(&ctx, aosp, &mb("main"), Staleness::MostRecent)
        .await?;
    assert_eq!(
        members,
        vec![dup],
        "duplicate edges must collapse to exactly one row"
    );
    assert_eq!(
        store
            .get_watermark(&ctx, aosp, Staleness::MostRecent)
            .await?,
        Some(9),
        "watermark advances once in the dedup replace transaction"
    );

    Ok(())
}

// 12. An empty edge set is a legitimate "clear membership" request: it deletes
//     the manifest branch's rows (skipping the INSERT) while still advancing the
//     watermark supplied in the same transaction.
#[mononoke::fbinit_test]
async fn test_replace_membership_empty_clears(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = new_store()?;
    let aosp = rid(1);

    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("main"),
            &[
                edge("platform/build", "aosp-main"),
                edge("platform/art", "aosp-main"),
            ],
            Some(5),
        )
        .await?;

    store
        .replace_membership(&ctx, aosp, &mb("main"), &[], Some(6))
        .await?;

    assert!(
        store
            .members_for_manifest_branch(&ctx, aosp, &mb("main"), Staleness::MostRecent)
            .await?
            .is_empty(),
        "an empty replace must clear the manifest branch's membership"
    );
    assert!(
        store
            .manifest_branches_for_repo(
                &ctx,
                &rn("platform/build"),
                &rb("aosp-main"),
                Staleness::MostRecent,
            )
            .await?
            .is_empty(),
        "former members must be gone from the reverse index"
    );
    assert_eq!(
        store
            .get_watermark(&ctx, aosp, Staleness::MostRecent)
            .await?,
        Some(6),
        "the watermark still advances when membership is cleared"
    );

    Ok(())
}

// 13. NEGATIVE: replace_membership with watermark=None must NOT disturb an
//     already-set watermark.
#[mononoke::fbinit_test]
async fn test_replace_membership_none_watermark_preserved(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = new_store()?;
    let aosp = rid(1);

    store.set_watermark(&ctx, aosp, 7).await?;
    store
        .replace_membership(
            &ctx,
            aosp,
            &mb("main"),
            &[edge("platform/build", "aosp-main")],
            None,
        )
        .await?;

    assert_eq!(
        store
            .get_watermark(&ctx, aosp, Staleness::MostRecent)
            .await?,
        Some(7),
        "a None watermark must leave the existing watermark untouched"
    );

    Ok(())
}

// 14. The in-memory Test double must be observationally equivalent to the SQL
//     store: the same sequence run against both yields identical reads. Future
//     consumers (backfill/tailer/reconciler/read-API) substitute the double in
//     their own tests, so this parity is the contract that keeps those honest.
#[mononoke::fbinit_test]
async fn test_sql_and_test_double_parity(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    async fn observe(
        ctx: &CoreContext,
        store: &dyn RepoManifestMapping,
    ) -> Result<(
        Vec<MembershipEdge>,
        Vec<(RepositoryId, ManifestBranch)>,
        Option<i64>,
    )> {
        let aosp = rid(1);
        let zephyr = rid(2);
        store
            .replace_membership(
                ctx,
                aosp,
                &mb("main"),
                &[
                    edge("platform/build", "aosp-main"),
                    edge("platform/art", "aosp-dev"),
                    // A duplicate edge must collapse identically in both impls.
                    edge("platform/build", "aosp-main"),
                ],
                Some(11),
            )
            .await?;
        store
            .replace_membership(
                ctx,
                zephyr,
                &mb("main"),
                &[edge("platform/build", "aosp-main")],
                None,
            )
            .await?;

        let forward = store
            .members_for_manifest_branch(ctx, aosp, &mb("main"), Staleness::MostRecent)
            .await?;
        let reverse = store
            .manifest_branches_for_repo(
                ctx,
                &rn("platform/build"),
                &rb("aosp-main"),
                Staleness::MostRecent,
            )
            .await?;
        let watermark = store
            .get_watermark(ctx, aosp, Staleness::MostRecent)
            .await?;
        Ok((forward, reverse, watermark))
    }

    let sql = new_store()?;
    let test_double = TestRepoManifestMapping::new();

    let sql_out = observe(&ctx, &sql).await?;
    let test_out = observe(&ctx, &test_double).await?;

    assert_eq!(
        sql_out, test_out,
        "the Test double must mirror the SQL store's observable semantics"
    );
    // Guard against a vacuous pass: the sequence really produces data.
    assert_eq!(
        sql_out.1,
        vec![(rid(1), mb("main")), (rid(2), mb("main"))],
        "platform/build@aosp-main fans out to both manifest repos"
    );
    assert_eq!(sql_out.2, Some(11));

    Ok(())
}

// 15. The Noop double reports empty/ok everywhere and never stores anything.
#[mononoke::fbinit_test]
async fn test_noop_double(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let noop = NoopRepoManifestMapping {};
    let aosp = rid(1);

    noop.replace_membership(
        &ctx,
        aosp,
        &mb("main"),
        &[edge("platform/build", "aosp-main")],
        Some(1),
    )
    .await?;
    noop.set_watermark(&ctx, aosp, 42).await?;

    assert!(
        noop.members_for_manifest_branch(&ctx, aosp, &mb("main"), Staleness::MostRecent)
            .await?
            .is_empty()
    );
    assert!(
        noop.manifest_branches_for_repo(
            &ctx,
            &rn("platform/build"),
            &rb("aosp-main"),
            Staleness::MostRecent,
        )
        .await?
        .is_empty()
    );
    assert_eq!(
        noop.get_watermark(&ctx, aosp, Staleness::MostRecent)
            .await?,
        None,
        "the Noop double stores nothing"
    );

    Ok(())
}

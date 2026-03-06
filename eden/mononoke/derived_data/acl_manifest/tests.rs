/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::Loadable;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_macros::mononoke;
use pretty_assertions::assert_eq;
use repo_blobstore::RepoBlobstoreRef;
use tests_utils::CreateCommitContext;

use crate::test_utils::*;

// ---------------------------------------------------------------------------
// Incremental derivation tests
// ---------------------------------------------------------------------------

/// Test incremental derivation: parent has no .slacl, child adds one.
#[mononoke::fbinit_test]
async fn test_derive_incremental_add(fb: FacebookInit) -> Result<()> {
    let result = setup_and_derive(
        fb,
        vec![
            vec![Change::Add("dir/file.txt", b"content")],
            vec![Change::Add("dir/.slacl", SLACL_PROJECT1)],
        ],
    )
    .await?;

    assert_eq!(Vec::<ExpectedNode>::new(), result.tree(0).await?);

    assert_eq!(
        vec![node(
            "dir",
            Some("REPO_REGION:repos/hg/fbsource/=project1"),
            vec![]
        )],
        result.last_tree().await?,
    );

    Ok(())
}

/// Test incremental derivation: parent has .slacl, child removes it.
#[mononoke::fbinit_test]
async fn test_derive_incremental_remove(fb: FacebookInit) -> Result<()> {
    let result = setup_and_derive(
        fb,
        vec![
            vec![
                Change::Add("dir/file.txt", b"content"),
                Change::Add("dir/.slacl", SLACL_PROJECT1),
            ],
            vec![Change::Delete("dir/.slacl")],
        ],
    )
    .await?;

    assert_eq!(
        vec![node(
            "dir",
            Some("REPO_REGION:repos/hg/fbsource/=project1"),
            vec![]
        )],
        result.tree(0).await?,
    );

    assert_eq!(Vec::<ExpectedNode>::new(), result.last_tree().await?);

    Ok(())
}

/// Test incremental derivation: changing .slacl content changes entry_blob_id.
#[mononoke::fbinit_test]
async fn test_derive_incremental_modify(fb: FacebookInit) -> Result<()> {
    let result = setup_and_derive(
        fb,
        vec![
            vec![
                Change::Add("dir/file.txt", b"content"),
                Change::Add("dir/.slacl", SLACL_PROJECT1),
            ],
            vec![Change::Add("dir/.slacl", SLACL_PROJECT2)],
        ],
    )
    .await?;

    assert_eq!(
        vec![node(
            "dir",
            Some("REPO_REGION:repos/hg/fbsource/=project2"),
            vec![]
        )],
        result.last_tree().await?,
    );

    // Verify entry_blob_ids differ
    let parent_entries =
        load_entries(&result.ctx, &result.repo, result.manifest_ids[0].inner_id()).await?;
    let child_entries =
        load_entries(&result.ctx, &result.repo, result.manifest_ids[1].inner_id()).await?;
    let parent_blob = expect_restricted(&result.ctx, &result.repo, &parent_entries[0].1.id).await?;
    let child_blob = expect_restricted(&result.ctx, &result.repo, &child_entries[0].1.id).await?;
    assert_ne!(
        parent_blob, child_blob,
        "modifying .slacl should produce a different entry_blob_id"
    );

    Ok(())
}

/// Test incremental: add nested restrictions to existing restriction roots.
/// Adds a restriction nested UNDER alpha, and a restriction ABOVE beta/deep.
#[mononoke::fbinit_test]
async fn test_incremental_add_nested_restriction(fb: FacebookInit) -> Result<()> {
    let result = setup_and_derive(
        fb,
        vec![
            vec![
                Change::Add("alpha/.slacl", SLACL_PROJECT1),
                Change::Add("alpha/file.txt", b"content"),
                Change::Add("beta/deep/.slacl", SLACL_PROJECT2),
                Change::Add("beta/deep/file.txt", b"content"),
            ],
            vec![
                Change::Add("alpha/inner/.slacl", SLACL_PROJECT2),
                Change::Add("alpha/inner/file.txt", b"content"),
                Change::Add("beta/.slacl", SLACL_PROJECT1),
            ],
        ],
    )
    .await?;

    assert_eq!(
        vec![
            node(
                "alpha",
                Some("REPO_REGION:repos/hg/fbsource/=project1"),
                vec![]
            ),
            node(
                "beta",
                None,
                vec![node(
                    "deep",
                    Some("REPO_REGION:repos/hg/fbsource/=project2"),
                    vec![]
                )]
            ),
        ],
        result.tree(0).await?,
    );

    assert_eq!(
        vec![
            node(
                "alpha",
                Some("REPO_REGION:repos/hg/fbsource/=project1"),
                vec![node(
                    "inner",
                    Some("REPO_REGION:repos/hg/fbsource/=project2"),
                    vec![]
                )]
            ),
            node(
                "beta",
                Some("REPO_REGION:repos/hg/fbsource/=project1"),
                vec![node(
                    "deep",
                    Some("REPO_REGION:repos/hg/fbsource/=project2"),
                    vec![]
                )]
            ),
        ],
        result.last_tree().await?,
    );

    Ok(())
}

/// Test incremental: remove nested restrictions from nested pairs.
/// Removes the nested restriction FROM alpha, and the parent restriction ABOVE beta/deep.
#[mononoke::fbinit_test]
async fn test_incremental_remove_nested_restriction(fb: FacebookInit) -> Result<()> {
    let result = setup_and_derive(
        fb,
        vec![
            vec![
                Change::Add("alpha/.slacl", SLACL_PROJECT1),
                Change::Add("alpha/inner/.slacl", SLACL_PROJECT2),
                Change::Add("alpha/inner/file.txt", b"content"),
                Change::Add("beta/.slacl", SLACL_PROJECT1),
                Change::Add("beta/deep/.slacl", SLACL_PROJECT2),
                Change::Add("beta/deep/file.txt", b"content"),
            ],
            vec![
                Change::Delete("alpha/inner/.slacl"),
                Change::Delete("beta/.slacl"),
            ],
        ],
    )
    .await?;

    assert_eq!(
        vec![
            node(
                "alpha",
                Some("REPO_REGION:repos/hg/fbsource/=project1"),
                vec![node(
                    "inner",
                    Some("REPO_REGION:repos/hg/fbsource/=project2"),
                    vec![]
                )]
            ),
            node(
                "beta",
                Some("REPO_REGION:repos/hg/fbsource/=project1"),
                vec![node(
                    "deep",
                    Some("REPO_REGION:repos/hg/fbsource/=project2"),
                    vec![]
                )]
            ),
        ],
        result.tree(0).await?,
    );

    assert_eq!(
        vec![
            node(
                "alpha",
                Some("REPO_REGION:repos/hg/fbsource/=project1"),
                vec![]
            ),
            node(
                "beta",
                None,
                vec![node(
                    "deep",
                    Some("REPO_REGION:repos/hg/fbsource/=project2"),
                    vec![]
                )]
            ),
        ],
        result.last_tree().await?,
    );

    Ok(())
}

/// Test incremental: modify nested restrictions in nested pairs.
/// Modifies the nested restriction IN alpha, and the parent restriction ABOVE beta/deep.
#[mononoke::fbinit_test]
async fn test_incremental_modify_nested_restriction(fb: FacebookInit) -> Result<()> {
    let result = setup_and_derive(
        fb,
        vec![
            vec![
                Change::Add("alpha/.slacl", SLACL_PROJECT1),
                Change::Add("alpha/inner/.slacl", SLACL_PROJECT2),
                Change::Add("alpha/inner/file.txt", b"content"),
                Change::Add("beta/.slacl", SLACL_PROJECT1),
                Change::Add("beta/deep/.slacl", SLACL_PROJECT2),
                Change::Add("beta/deep/file.txt", b"content"),
            ],
            vec![
                Change::Add("alpha/inner/.slacl", SLACL_PROJECT1),
                Change::Add("beta/.slacl", SLACL_PROJECT2),
            ],
        ],
    )
    .await?;

    // Before: alpha/inner=project2, beta=project1
    assert_eq!(
        vec![
            node(
                "alpha",
                Some("REPO_REGION:repos/hg/fbsource/=project1"),
                vec![node(
                    "inner",
                    Some("REPO_REGION:repos/hg/fbsource/=project2"),
                    vec![]
                )]
            ),
            node(
                "beta",
                Some("REPO_REGION:repos/hg/fbsource/=project1"),
                vec![node(
                    "deep",
                    Some("REPO_REGION:repos/hg/fbsource/=project2"),
                    vec![]
                )]
            ),
        ],
        result.tree(0).await?,
    );

    // After: alpha/inner changed to project1, beta changed to project2
    assert_eq!(
        vec![
            node(
                "alpha",
                Some("REPO_REGION:repos/hg/fbsource/=project1"),
                vec![node(
                    "inner",
                    Some("REPO_REGION:repos/hg/fbsource/=project1"),
                    vec![]
                )]
            ),
            node(
                "beta",
                Some("REPO_REGION:repos/hg/fbsource/=project2"),
                vec![node(
                    "deep",
                    Some("REPO_REGION:repos/hg/fbsource/=project2"),
                    vec![]
                )]
            ),
        ],
        result.last_tree().await?,
    );

    Ok(())
}

/// Test content-addressing: identical .slacl files at different paths
/// should produce the same AclManifestEntryBlobId.
#[mononoke::fbinit_test]
async fn test_entry_blob_content_addressing(fb: FacebookInit) -> Result<()> {
    let result = setup_and_derive(
        fb,
        vec![vec![
            Change::Add("alpha/file.txt", b"content a"),
            Change::Add("alpha/.slacl", SLACL_PROJECT1),
            Change::Add("beta/file.txt", b"content b"),
            Change::Add("beta/.slacl", SLACL_PROJECT1),
        ]],
    )
    .await?;

    let mut entries = load_entries(&result.ctx, &result.repo, result.last_id().inner_id()).await?;
    entries.sort_by(|(a, _), (b, _)| a.cmp(b));

    let alpha_blob = expect_restricted(&result.ctx, &result.repo, &entries[0].1.id).await?;
    let beta_blob = expect_restricted(&result.ctx, &result.repo, &entries[1].1.id).await?;

    assert_eq!(
        alpha_blob, beta_blob,
        "identical .slacl content should produce the same entry_blob_id"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Incremental derivation edge-case tests
// ---------------------------------------------------------------------------

/// Test incremental: remove a restriction root that has restricted descendants.
/// The node should become a waypoint, not be pruned.
#[mononoke::fbinit_test]
async fn test_incremental_remove_keeps_waypoint(fb: FacebookInit) -> Result<()> {
    let result = setup_and_derive(
        fb,
        vec![
            vec![
                Change::Add("foo/.slacl", SLACL_PROJECT1),
                Change::Add("foo/bar/.slacl", SLACL_PROJECT2),
                Change::Add("foo/bar/file.txt", b"content"),
            ],
            vec![Change::Delete("foo/.slacl")],
        ],
    )
    .await?;

    // Parent commit: foo was restricted with project1
    assert_eq!(
        vec![node(
            "foo",
            Some("REPO_REGION:repos/hg/fbsource/=project1"),
            vec![node(
                "bar",
                Some("REPO_REGION:repos/hg/fbsource/=project2"),
                vec![]
            )],
        )],
        result.tree(0).await?,
    );

    // Child commit: foo is now an unrestricted waypoint
    assert_eq!(
        vec![node(
            "foo",
            None, // foo is now unrestricted (waypoint)
            vec![node(
                "bar",
                Some("REPO_REGION:repos/hg/fbsource/=project2"),
                vec![]
            )],
        )],
        result.last_tree().await?,
    );

    Ok(())
}

/// Test incremental: unchanged subtree is reused by ID.
#[mononoke::fbinit_test]
async fn test_incremental_reuses_unchanged_subtree(fb: FacebookInit) -> Result<()> {
    let result = setup_and_derive(
        fb,
        vec![
            vec![
                Change::Add("alpha/.slacl", SLACL_PROJECT1),
                Change::Add("alpha/file.txt", b"content"),
                Change::Add("beta/.slacl", SLACL_PROJECT2),
                Change::Add("beta/file.txt", b"content"),
            ],
            vec![Change::Add("alpha/.slacl", SLACL_PROJECT2)],
        ],
    )
    .await?;

    let parent_entries =
        load_entries(&result.ctx, &result.repo, result.manifest_ids[0].inner_id()).await?;
    let child_entries =
        load_entries(&result.ctx, &result.repo, result.manifest_ids[1].inner_id()).await?;

    let parent_beta = parent_entries
        .iter()
        .find(|(name, _)| name.as_ref() == b"beta")
        .map(|(_, e)| e.id)
        .ok_or_else(|| anyhow::anyhow!("beta not found in parent"))?;

    let child_beta = child_entries
        .iter()
        .find(|(name, _)| name.as_ref() == b"beta")
        .map(|(_, e)| e.id)
        .ok_or_else(|| anyhow::anyhow!("beta not found in child"))?;

    assert_eq!(
        parent_beta, child_beta,
        "unchanged beta subtree should be reused by ID"
    );

    Ok(())
}

/// Test incremental: no ACL changes → parent manifest reused entirely.
#[mononoke::fbinit_test]
async fn test_incremental_no_changes_reuses_parent(fb: FacebookInit) -> Result<()> {
    let result = setup_and_derive(
        fb,
        vec![
            vec![
                Change::Add("dir/.slacl", SLACL_PROJECT1),
                Change::Add("dir/file.txt", b"content"),
            ],
            vec![Change::Add("dir/file.txt", b"updated content")],
        ],
    )
    .await?;

    assert_eq!(
        result.manifest_ids[0].inner_id(),
        result.manifest_ids[1].inner_id(),
        "no ACL changes should reuse parent manifest exactly"
    );

    Ok(())
}

/// Test incremental: implicit deletes (adding a file at a directory path).
/// alpha/sub has nested restriction roots (sub + sub/inner); beta has nested
/// restriction roots (beta + beta/deep). The child commit implicitly deletes
/// the nested root in alpha (alpha/sub/inner) and implicitly deletes beta
/// entirely by adding regular files at those paths.
#[mononoke::fbinit_test]
async fn test_incremental_implicit_delete(fb: FacebookInit) -> Result<()> {
    let result = setup_and_derive(
        fb,
        vec![
            vec![
                Change::Add("alpha/sub/.slacl", SLACL_PROJECT1),
                Change::Add("alpha/sub/inner/.slacl", SLACL_PROJECT2),
                Change::Add("alpha/sub/inner/file.txt", b"content"),
                Change::Add("beta/.slacl", SLACL_PROJECT1),
                Change::Add("beta/deep/.slacl", SLACL_PROJECT2),
                Change::Add("beta/deep/file.txt", b"content"),
            ],
            vec![
                // Implicitly delete alpha/sub/inner dir by adding file at that path
                Change::Add("alpha/sub/inner", b"now a file"),
                // Implicitly delete beta dir by adding file at that path
                Change::Add("beta", b"now a file"),
            ],
        ],
    )
    .await?;

    // Before: nested restrictions in both subtrees
    assert_eq!(
        vec![
            node(
                "alpha",
                None,
                vec![node(
                    "sub",
                    Some("REPO_REGION:repos/hg/fbsource/=project1"),
                    vec![node(
                        "inner",
                        Some("REPO_REGION:repos/hg/fbsource/=project2"),
                        vec![]
                    )]
                )]
            ),
            node(
                "beta",
                Some("REPO_REGION:repos/hg/fbsource/=project1"),
                vec![node(
                    "deep",
                    Some("REPO_REGION:repos/hg/fbsource/=project2"),
                    vec![]
                )]
            ),
        ],
        result.tree(0).await?,
    );

    // After: alpha/sub/inner gone (sub remains), beta gone entirely
    assert_eq!(
        vec![node(
            "alpha",
            None,
            vec![node(
                "sub",
                Some("REPO_REGION:repos/hg/fbsource/=project1"),
                vec![]
            )]
        )],
        result.last_tree().await?,
    );

    Ok(())
}

/// Test merge: two branches add restriction roots in different directories.
/// The merge combines both without conflict.
#[mononoke::fbinit_test]
async fn test_merge_no_conflict(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("shared/file.txt", "content")
        .commit()
        .await?;

    // Left branch: adds alpha restriction
    let left = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("alpha/.slacl", SLACL_PROJECT1)
        .add_file("alpha/file.txt", "content")
        .commit()
        .await?;

    // Right branch: adds beta restriction
    let right = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("beta/.slacl", SLACL_PROJECT2)
        .add_file("beta/file.txt", "content")
        .commit()
        .await?;

    // Merge combines both branches
    let merge = CreateCommitContext::new(&ctx, &repo, vec![left, right])
        .commit()
        .await?;

    let merge_id = derive(&ctx, &repo, merge).await?;
    let tree = actual_tree(&ctx, &repo, merge_id.inner_id()).await?;

    assert_eq!(
        vec![
            node(
                "alpha",
                Some("REPO_REGION:repos/hg/fbsource/=project1"),
                vec![]
            ),
            node(
                "beta",
                Some("REPO_REGION:repos/hg/fbsource/=project2"),
                vec![]
            ),
        ],
        tree,
    );

    Ok(())
}

/// Test merge: two branches add conflicting .slacl at the same path.
/// The merge commit resolves the conflict; the derived manifest reflects
/// the merge commit's file contents, not either parent's.
#[mononoke::fbinit_test]
async fn test_merge_conflicting_restriction(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/file.txt", "content")
        .commit()
        .await?;

    // Left: adds dir/.slacl = project1
    let left = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("dir/.slacl", SLACL_PROJECT1)
        .commit()
        .await?;

    // Right: adds dir/.slacl = project2 (conflict with left)
    let right = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("dir/.slacl", SLACL_PROJECT2)
        .commit()
        .await?;

    // Merge resolves conflict: picks project2
    let merge = CreateCommitContext::new(&ctx, &repo, vec![left, right])
        .add_file("dir/.slacl", SLACL_PROJECT2)
        .commit()
        .await?;

    let merge_id = derive(&ctx, &repo, merge).await?;
    let tree = actual_tree(&ctx, &repo, merge_id.inner_id()).await?;

    assert_eq!(
        vec![node(
            "dir",
            Some("REPO_REGION:repos/hg/fbsource/=project2"),
            vec![]
        )],
        tree,
    );

    Ok(())
}
// TODO(T248660053): add tests with subtree copies and merges.

// TODO(T248660053): add tests to cover derivation complexity is O(restriction root changes * depth)

// ---------------------------------------------------------------------------
// Additional test cases
// ---------------------------------------------------------------------------

/// Test root-level .slacl: a restriction file at the repo root itself.
#[mononoke::fbinit_test]
async fn test_derive_root_level_slacl(fb: FacebookInit) -> Result<()> {
    let result = setup_and_derive(
        fb,
        vec![vec![
            Change::Add("file.txt", b"content"),
            Change::Add(".slacl", SLACL_PROJECT1),
        ]],
    )
    .await?;

    // The root manifest itself should be restricted (no waypoint entries,
    // since the root IS the restriction root).
    let root_id = result.last_id().inner_id();
    let blob_id = expect_restricted(&result.ctx, &result.repo, root_id).await?;

    let blob = blob_id
        .load(&result.ctx, result.repo.repo_blobstore())
        .await?;
    assert_eq!(
        blob.repo_region_acl,
        "REPO_REGION:repos/hg/fbsource/=project1"
    );

    Ok(())
}

/// Test permission_request_group: an ACL file with explicit group.
#[mononoke::fbinit_test]
async fn test_derive_permission_request_group(fb: FacebookInit) -> Result<()> {
    let result = setup_and_derive(
        fb,
        vec![vec![
            Change::Add("dir/file.txt", b"content"),
            Change::Add("dir/.slacl", SLACL_PROJECT1_WITH_GROUP),
        ]],
    )
    .await?;

    let tree = result.last_tree().await?;
    assert_eq!(tree.len(), 1);
    assert_eq!(
        tree[0].permission_request_group.as_deref(),
        Some("GROUP:my_amp_group"),
    );

    Ok(())
}

/// Test large fan-out: many restriction roots at the same directory level.
#[mononoke::fbinit_test]
async fn test_derive_large_fan_out(fb: FacebookInit) -> Result<()> {
    let changes: Vec<Change<'_>> = (0..20)
        .flat_map(|i| {
            let dir = format!("dir_{:03}", i);
            // Leak the strings so they live long enough as &str
            let slacl_path: &'static str = Box::leak(format!("{dir}/.slacl").into_boxed_str());
            let file_path: &'static str = Box::leak(format!("{dir}/file.txt").into_boxed_str());
            vec![
                Change::Add(slacl_path, SLACL_PROJECT1),
                Change::Add(file_path, b"content"),
            ]
        })
        .collect();

    let result = setup_and_derive(fb, vec![changes]).await?;
    let tree = result.last_tree().await?;

    assert_eq!(tree.len(), 20, "should have 20 restriction roots");
    assert!(
        tree.iter().all(|n| n.is_restricted),
        "all roots should be restricted"
    );

    Ok(())
}

/// Test derivation idempotency: deriving the same changeset twice produces
/// identical manifest IDs.
#[mononoke::fbinit_test]
async fn test_derive_idempotency(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_id = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/.slacl", SLACL_PROJECT1)
        .add_file("dir/file.txt", "content")
        .add_file("other/nested/.slacl", SLACL_PROJECT2)
        .add_file("other/nested/file.txt", "content")
        .commit()
        .await?;

    let first = derive(&ctx, &repo, cs_id).await?;
    let second = derive(&ctx, &repo, cs_id).await?;

    assert_eq!(
        first.inner_id(),
        second.inner_id(),
        "deriving the same changeset twice should produce identical manifest IDs"
    );

    Ok(())
}

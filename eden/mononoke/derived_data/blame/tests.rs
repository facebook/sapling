/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Error;
use anyhow::anyhow;
use blobstore::KeyedBlobstore;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use borrowed::borrowed;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use derivation_queue_thrift::DerivationPriority;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use history_manifest::RootHistoryManifestDirectoryId;
use manifest::ManifestOps;
use maplit::btreemap;
use maplit::hashmap;
use metaconfig_types::BlameVersion;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use mononoke_types::blame_v2::BlameRejected;
use mononoke_types::blame_v2::BlameV2;
use mononoke_types::blame_v3::BlameV3Id;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use test_repo_factory::TestRepoFactory;
use tests_utils::CreateCommitContext;
use tests_utils::create_commit;
use tests_utils::store_files;
use tests_utils::store_rename;

use crate::BlameError;
use crate::RootBlameV2;
use crate::derive_from_predecessor_v3::derive_blame_v3_from_predecessor;
use crate::fetch_blame_v2;
use crate::fetch_blame_v3;

#[facet::container]
struct TestRepo {
    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,
    #[facet]
    bookmarks: dyn Bookmarks,
    #[facet]
    repo_blobstore: RepoBlobstore,
    #[facet]
    repo_derived_data: RepoDerivedData,
    #[facet]
    filestore_config: FilestoreConfig,
    #[facet]
    commit_graph: CommitGraph,
    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,
    #[facet]
    repo_identity: RepoIdentity,
}

// File with multiple changes and a merge
const F0: &[&str] = &[
    // c0
    r#"|
1 0
1 1
"#,
    // c1
    r#"|
2 0
1 0
2 1
"#,
    // c2
    r#"|
2 0
1 0
3 0
3 1
2 1
3 2
"#,
    // c3
    r#"|
1 0
1 1
3 2
4 0
"#,
    // c4
    r#"|
2 0
1 0
3 0
3 1
2 1
3 2
4 0
"#,
];

const F0_AT_C4: &str = r#"c0: |
c1: 2 0
c0: 1 0
c2: 3 0
c2: 3 1
c1: 2 1
c2: 3 2
c3: 4 0
"#;

// file with multiple change only in one parent and a merge
const F1: &[&str] = &[
    // c0
    r#"|
1 0
1 1
"#,
    // c3
    r#"|
1 0
4 0
1 1
"#,
];

const F1_AT_C4: &str = r#"c0: |
c0: 1 0
c3: 4 0
c0: 1 1
"#;

// renamed file
const F2: &[&str] = &[
    // c0 as _f2
    r#"|
1 0
1 1
"#,
    // c1 as _f2 => f2
    r#"|
1 0
2 0
1 1
"#,
    // c3 as new f2
    r#"|
1 0
4 0
1 1
"#,
    // c4 as f2
    r#"|
5 0
1 0
2 0
4 0
1 1
"#,
];

const F2_AT_C4: &str = r#"c0: |
c4: 5 0
c0: 1 0
c1: 2 0
c3: 4 0
c0: 1 1
"#;

#[mononoke::fbinit_test]
async fn test_blame_v2(fb: FacebookInit) -> Result<(), Error> {
    test_blame_version(fb, BlameVersion::V2).await
}

#[mononoke::fbinit_test]
async fn test_blame_v3(fb: FacebookInit) -> Result<(), Error> {
    test_blame_version(fb, BlameVersion::V3).await
}

async fn fetch_blame_for_version(
    ctx: &CoreContext,
    repo: &TestRepo,
    version: BlameVersion,
    csid: ChangesetId,
    path: NonRootMPath,
) -> Result<BlameV2, Error> {
    match version {
        BlameVersion::V2 => Ok(fetch_blame_v2(ctx, repo, csid, path).await?.0),
        BlameVersion::V3 => Ok(fetch_blame_v3(ctx, repo, csid, path).await?.0),
    }
}

async fn test_blame_version(fb: FacebookInit, version: BlameVersion) -> Result<(), Error> {
    // Commits structure
    //
    //   0
    //  / \
    // 1   3
    // |   |
    // 2   |
    //  \ /
    //   4
    //
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = TestRepoFactory::new(fb)?
        .with_config_override(|config| {
            config
                .derived_data_config
                .get_active_config_mut()
                .expect("No enabled derived data types config")
                .blame_version = version
        })
        .build()
        .await?;
    borrowed!(ctx, repo);

    let c0 = create_commit(
        ctx.clone(),
        repo,
        vec![],
        store_files(
            ctx,
            btreemap! {
                "f0" => Some(F0[0]),
                "f1" => Some(F1[0]),
                "_f2" => Some(F2[0]),
            },
            repo,
        )
        .await,
    )
    .await;

    let mut c1_changes = store_files(ctx, btreemap! {"f0" => Some(F0[1])}, repo).await;
    let (f2_path, f2_change) =
        store_rename(ctx, (NonRootMPath::new("_f2")?, c0), "f2", F2[1], repo).await;
    c1_changes.insert(f2_path, f2_change);
    let c1 = create_commit(ctx.clone(), repo, vec![c0], c1_changes).await;

    let c2 = create_commit(
        ctx.clone(),
        repo,
        vec![c1],
        store_files(ctx, btreemap! {"f0" => Some(F0[2])}, repo).await,
    )
    .await;

    let c3 = create_commit(
        ctx.clone(),
        repo,
        vec![c0],
        store_files(
            ctx,
            btreemap! {
                "f0" => Some(F0[3]),
                "f1" => Some(F1[1]),
                "f2" => Some(F2[2]),
            },
            repo,
        )
        .await,
    )
    .await;

    let c4 = create_commit(
        ctx.clone(),
        repo,
        vec![c2, c3],
        store_files(
            ctx,
            btreemap! {
                "f0" => Some(F0[4]),
                "f1" => Some(F1[1]), // did not change after c3
                "f2" => Some(F2[3]),
            },
            repo,
        )
        .await,
    )
    .await;

    let names = hashmap! {
        c0 => "c0",
        c1 => "c1",
        c2 => "c2",
        c3 => "c3",
        c4 => "c4",
    };

    let blame = fetch_blame_for_version(ctx, repo, version, c4, NonRootMPath::new("f0")?).await?;
    assert_eq!(annotate(F0[4], blame, &names)?, F0_AT_C4);

    let blame = fetch_blame_for_version(ctx, repo, version, c4, NonRootMPath::new("f1")?).await?;
    assert_eq!(annotate(F1[1], blame, &names)?, F1_AT_C4);

    let blame = fetch_blame_for_version(ctx, repo, version, c4, NonRootMPath::new("f2")?).await?;
    assert_eq!(annotate(F2[3], blame, &names)?, F2_AT_C4);

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_blame_size_rejected_v2(fb: FacebookInit) -> Result<(), Error> {
    test_blame_size_rejected_version(fb, BlameVersion::V2).await
}

#[mononoke::fbinit_test]
async fn test_blame_size_rejected_v3(fb: FacebookInit) -> Result<(), Error> {
    test_blame_size_rejected_version(fb, BlameVersion::V3).await
}

async fn test_blame_size_rejected_version(
    fb: FacebookInit,
    version: BlameVersion,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await.unwrap();
    borrowed!(ctx, repo);
    let file1 = "file1";
    let content = "content";
    let c1 = CreateCommitContext::new_root(ctx, &repo)
        .add_file(file1, content)
        .commit()
        .await?;

    // Default file size is 10MiB, so blame should be computed
    // without problems.
    let blame = fetch_blame_for_version(ctx, repo, version, c1, NonRootMPath::new(file1)?).await?;
    let _ = blame.ranges()?;

    let repo: TestRepo = TestRepoFactory::new(fb)?
        .with_config_override(|config| {
            config
                .derived_data_config
                .get_active_config_mut()
                .expect("No enabled derived data types config")
                .blame_version = version;
            config
                .derived_data_config
                .get_active_config_mut()
                .expect("No enabled derived data types config")
                .blame_filesize_limit = Some(4);
        })
        .build()
        .await?;

    let file2 = "file2";
    let c2 = CreateCommitContext::new_root(ctx, &repo)
        .add_file(file2, content)
        .commit()
        .await?;

    // This repo has a decreased limit, so derivation should fail now
    let blame = fetch_blame_for_version(ctx, &repo, version, c2, NonRootMPath::new(file2)?).await?;

    match blame.ranges() {
        Err(BlameRejected::TooBig) => {}
        _ => {
            return Err(anyhow!("unexpected result"));
        }
    }

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_blame_copy_source_v2(fb: FacebookInit) -> Result<(), Error> {
    test_blame_copy_source_version(fb, BlameVersion::V2).await
}

#[mononoke::fbinit_test]
async fn test_blame_copy_source_v3(fb: FacebookInit) -> Result<(), Error> {
    test_blame_copy_source_version(fb, BlameVersion::V3).await
}

async fn test_blame_copy_source_version(
    fb: FacebookInit,
    version: BlameVersion,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = TestRepoFactory::new(fb)?
        .with_config_override(|config| {
            config
                .derived_data_config
                .get_active_config_mut()
                .expect("No enabled derived data types config")
                .blame_version = version
        })
        .build()
        .await?;
    borrowed!(ctx, repo);

    let c1 = CreateCommitContext::new_root(ctx, &repo)
        .add_file("file1", "one\ntwo\nthree\n")
        .add_file("file2", "zero\none\ntwo\nfour\n")
        .commit()
        .await?;

    let data = "none\none\ntwo\nthree\n";
    let c2 = CreateCommitContext::new(ctx, &repo, vec![c1])
        .add_file_with_copy_info("file1", data, (c1, "file2"))
        .commit()
        .await?;

    let blame =
        fetch_blame_for_version(ctx, repo, version, c2, NonRootMPath::new("file1")?).await?;
    let lines = blame
        .lines()?
        .map(|line| (line.changeset_id, line.path.to_string(), line.origin_offset))
        .collect::<Vec<_>>();

    // The "one" and "two" lines are blamed to the copy source, and not the
    // parent.  The "three" line blames to the commit that performed the copy,
    // and not the parent.
    assert_eq!(
        lines,
        vec![
            (&c2, "file1".to_string(), 0),
            (&c1, "file2".to_string(), 1),
            (&c1, "file2".to_string(), 2),
            (&c2, "file1".to_string(), 3),
        ]
    );
    Ok(())
}

/// Derive blame V2 and V3 for the same repo and verify that both versions
/// produce identical blame ranges for every file at every commit.
#[mononoke::fbinit_test]
async fn test_blame_v2_v3_produce_identical_results(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    // Build repo with V2 config (V3 can always be derived independently).
    let repo: TestRepo = TestRepoFactory::new(fb)?
        .with_config_override(|config| {
            config
                .derived_data_config
                .get_active_config_mut()
                .expect("No enabled derived data types config")
                .blame_version = BlameVersion::V2
        })
        .build()
        .await?;
    borrowed!(ctx, repo);

    // Build a commit graph that exercises many blame scenarios:
    //
    //   c0 (root: file, stable, to_delete, to_rename)
    //    │
    //   c1 (modify file, delete to_delete, rename to_rename -> renamed)
    //    │
    //   c2 (modify file again, re-create to_delete with new content)
    //    │    c3 (independent root: file, only_c3)
    //    │   /
    //   c4 (merge c2+c3, resolve file, add merged_new)
    //    │
    //   c5 (empty commit — no file changes)

    // c0: root commit with multiple files.
    let c0 = CreateCommitContext::new_root(ctx, repo)
        .add_file("file", "line1\nline2\nline3\n")
        .add_file("stable", "never\nchanges\n")
        .add_file("to_delete", "will be deleted\n")
        .add_file("to_rename", "original\ncontent\n")
        .commit()
        .await?;

    // c1: modify, delete, rename.
    let c1 = CreateCommitContext::new(ctx, repo, vec![c0])
        .add_file("file", "line1\nmodified\nline3\n")
        .delete_file("to_delete")
        .add_file_with_copy_info(
            "renamed",
            "original\nnew_line\ncontent\n",
            (c0, "to_rename"),
        )
        .commit()
        .await?;

    // c2: modify again, re-create previously deleted file.
    let c2 = CreateCommitContext::new(ctx, repo, vec![c1])
        .add_file("file", "line1\nmodified\nline3\nextra\n")
        .add_file("to_delete", "resurrected content\n")
        .commit()
        .await?;

    // c3: independent root (simulates a branch with no common ancestor).
    let c3 = CreateCommitContext::new_root(ctx, repo)
        .add_file("file", "completely\ndifferent\n")
        .add_file("only_c3", "unique to c3\n")
        .commit()
        .await?;

    // c4: merge c2 and c3. Resolve all conflicts explicitly.
    let c4 = CreateCommitContext::new(ctx, repo, vec![c2, c3])
        .add_file("file", "line1\nmodified\nline3\nextra\nfrom_c3\n")
        .add_file("only_c3", "unique to c3\n")
        .add_file("merged_new", "brand new in merge\n")
        .add_file("stable", "never\nchanges\n")
        .add_file("renamed", "original\nnew_line\ncontent\n")
        .add_file("to_delete", "resurrected content\n")
        .commit()
        .await?;

    // c5: empty commit (no file changes) — blame should be identical to c4.
    let c5 = CreateCommitContext::new(ctx, repo, vec![c4])
        .commit()
        .await?;

    // Compare blame at each commit for files present there.
    let files_at = vec![
        // Root commit: new files, no parents.
        (c0, vec!["file", "stable", "to_delete", "to_rename"]),
        // Linear: modify, delete, rename.
        (c1, vec!["file", "stable", "renamed"]),
        // Linear: modify, re-create deleted file.
        (c2, vec!["file", "stable", "renamed", "to_delete"]),
        // Independent root.
        (c3, vec!["file", "only_c3"]),
        // Merge: all files from both parents, plus new.
        (
            c4,
            vec![
                "file",
                "stable",
                "renamed",
                "to_delete",
                "only_c3",
                "merged_new",
            ],
        ),
        // Empty commit: identical to parent.
        (
            c5,
            vec![
                "file",
                "stable",
                "renamed",
                "to_delete",
                "only_c3",
                "merged_new",
            ],
        ),
    ];

    for (csid, files) in &files_at {
        for file in files {
            let path = NonRootMPath::new(file)?;
            let blame_v2 =
                fetch_blame_for_version(ctx, repo, BlameVersion::V2, *csid, path.clone()).await?;
            let blame_v3 =
                fetch_blame_for_version(ctx, repo, BlameVersion::V3, *csid, path.clone()).await?;

            let v2_ranges: Vec<_> = blame_v2
                .ranges()?
                .map(|r| (r.csid, r.offset, r.length, r.origin_offset, r.path.clone()))
                .collect();
            let v3_ranges: Vec<_> = blame_v3
                .ranges()?
                .map(|r| (r.csid, r.offset, r.length, r.origin_offset, r.path.clone()))
                .collect();

            assert_eq!(
                v2_ranges, v3_ranges,
                "Blame V2 and V3 differ for {file} at {csid:?}",
            );
        }
    }

    Ok(())
}

/// Build the commit graph used by the predecessor-derivation tests, returning
/// each commit with the paths live at it.
///
/// `dir/shared` is the interesting path: deleted on one merge parent, kept on
/// the other, re-stated by the merge. There the history manifest reuses the
/// surviving parent's file node while unodes mint a fresh merge unode, so the
/// two disagree about whether the path is new at the merge.
async fn build_predecessor_test_graph(
    ctx: &CoreContext,
    repo: &TestRepo,
) -> Result<Vec<(ChangesetId, Vec<&'static str>)>, Error> {
    // Paths sit at several depths so the walk has to recurse and pair trees.
    // `gone/` is emptied on one branch and stays empty: the history manifest
    // collapses a fully deleted directory into a `DeletedNode` where the unode
    // manifest just drops it.
    //
    //        c0 (file, stable, to_copy, dir/{nested,shared}, dir/sub/deep, gone/only)
    //        ├────────────────────┐
    //       c1                   c2 (modify file, to_copy, dir/sub/deep)
    //        │ (delete dir/shared  │
    //        │  and gone/only;     │
    //        │  copy to_copy)      │
    //        └──────────┬──────────┘
    //                  c3 (merge; re-states dir/shared with c2's content,
    //                      keeps gone/only deleted)
    //                   │
    //                  c4 (empty commit — no file changes)
    let c0 = CreateCommitContext::new_root(ctx, repo)
        .add_file("file", "line1\nline2\nline3\n")
        .add_file("stable", "never\nchanges\n")
        .add_file("to_copy", "original\ncontent\n")
        .add_file("dir/nested", "nested\ncontent\n")
        .add_file("dir/shared", "shared\ncontent\n")
        .add_file("dir/sub/deep", "deep\ncontent\n")
        .add_file("gone/only", "will be deleted\n")
        .commit()
        .await?;

    // `add_file_with_copy_info` records copy-from without removing the source,
    // so this is a copy: `to_copy` stays live alongside `dir/copied`.
    let c1 = CreateCommitContext::new(ctx, repo, vec![c0])
        .add_file("file", "line1\nmodified\nline3\n")
        .delete_file("dir/shared")
        .delete_file("gone/only")
        .add_file_with_copy_info(
            "dir/copied",
            "original\nnew_line\ncontent\n",
            (c0, "to_copy"),
        )
        .commit()
        .await?;

    let c2 = CreateCommitContext::new(ctx, repo, vec![c0])
        .add_file("file", "line1\nline2\nfrom_c2\n")
        .add_file("to_copy", "original\nedited\n")
        .add_file("dir/sub/deep", "deep\nedited\n")
        .commit()
        .await?;

    let c3 = CreateCommitContext::new(ctx, repo, vec![c1, c2])
        .add_file("file", "line1\nmodified\nfrom_c2\n")
        .add_file("to_copy", "original\nedited\n")
        .add_file("dir/shared", "shared\ncontent\n")
        .add_file("dir/sub/deep", "deep\nedited\n")
        .delete_file("gone/only")
        .commit()
        .await?;

    let c4 = CreateCommitContext::new(ctx, repo, vec![c3])
        .commit()
        .await?;

    let merged = vec![
        "file",
        "stable",
        "to_copy",
        "dir/nested",
        "dir/shared",
        "dir/sub/deep",
        "dir/copied",
    ];
    Ok(vec![
        (
            c0,
            vec![
                "file",
                "stable",
                "to_copy",
                "dir/nested",
                "dir/shared",
                "dir/sub/deep",
                "gone/only",
            ],
        ),
        (
            c1,
            vec![
                "file",
                "stable",
                "to_copy",
                "dir/nested",
                "dir/sub/deep",
                "dir/copied",
            ],
        ),
        (
            c2,
            vec![
                "file",
                "stable",
                "to_copy",
                "dir/nested",
                "dir/shared",
                "dir/sub/deep",
                "gone/only",
            ],
        ),
        (c3, merged.clone()),
        (c4, merged),
    ])
}

/// Blame v3 transcoded from its blame v2 predecessor must land the same blame
/// under the same path as blame v2 itself.
///
/// Exercises the manifest join: a partial walk leaves a path with no v3 blob,
/// a mispaired one gives a path some other file's blame.
#[mononoke::fbinit_test]
async fn test_blame_v3_from_predecessor(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = TestRepoFactory::new(fb)?.build().await?;
    borrowed!(ctx, repo);

    let files_at = build_predecessor_test_graph(ctx, repo).await?;

    let blobstore = repo.repo_blobstore().boxed();
    let derived_data = repo.repo_derived_data();

    for (csid, files) in &files_at {
        // Derive only the predecessors, then transcode.  Blame v3 is never
        // derived normally in this test, so the blobs read back below are the
        // transcoded ones.
        let blame_v2 = derived_data
            .derive::<RootBlameV2>(ctx, *csid, DerivationPriority::LOW)
            .await?;
        let root_manifest = derived_data
            .derive::<RootHistoryManifestDirectoryId>(ctx, *csid, DerivationPriority::LOW)
            .await?;
        derive_blame_v3_from_predecessor(ctx, &blobstore, blame_v2, root_manifest).await?;

        for file in files {
            let path = NonRootMPath::new(file)?;
            let hm_file_id = root_manifest
                .into_history_manifest_directory_id()
                .find_entry(ctx.clone(), blobstore.clone(), path.clone().into())
                .await?
                .ok_or_else(|| anyhow!("{path} missing from history manifest at {csid:?}"))?
                .into_leaf()
                .ok_or_else(|| anyhow!("{path} is a directory at {csid:?}"))?;

            let from_predecessor = BlameV3Id::from(hm_file_id).load(ctx, &blobstore).await?;
            let (expected, _) = fetch_blame_v2(ctx, repo, *csid, path.clone()).await?;

            let from_predecessor_ranges: Vec<_> = from_predecessor
                .ranges()?
                .map(|r| (r.csid, r.offset, r.length, r.origin_offset, r.path.clone()))
                .collect();
            let expected_ranges: Vec<_> = expected
                .ranges()?
                .map(|r| (r.csid, r.offset, r.length, r.origin_offset, r.path.clone()))
                .collect();

            assert_eq!(
                from_predecessor_ranges, expected_ranges,
                "Blame v3 from predecessor differs from blame v2 for {file} at {csid:?}",
            );
        }
    }

    Ok(())
}

/// Deleted paths must be absent from both manifests, including when the
/// deletion empties a whole directory.
///
/// The positional pairing depends on it: the history manifest keeps deleted
/// paths on disk and only its `Manifest` impl hides them, so if that filtering
/// changed the two leaf streams would drift apart.
#[mononoke::fbinit_test]
async fn test_predecessor_graph_deletions_are_filtered(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = TestRepoFactory::new(fb)?.build().await?;
    borrowed!(ctx, repo);

    let graph = build_predecessor_test_graph(ctx, repo).await?;
    let c0 = graph[0].0;
    let c1 = graph[1].0;
    let c3 = graph[3].0;

    for path in ["dir/shared", "gone/only"] {
        let path = NonRootMPath::new(path)?;
        fetch_blame_v2(ctx, repo, c0, path.clone())
            .await
            .with_context(|| format!("{path} should be live at c0 in the unode manifest"))?;
        fetch_blame_v3(ctx, repo, c0, path.clone())
            .await
            .with_context(|| format!("{path} should be live at c0 in the history manifest"))?;
    }

    // `gone` covers the directory itself, which only the history manifest has
    // an entry for once it is empty.
    let deleted = [
        (c1, "dir/shared"),
        (c1, "gone/only"),
        (c1, "gone"),
        (c3, "gone/only"),
        (c3, "gone"),
    ];
    for (csid, path) in deleted {
        let path = NonRootMPath::new(path)?;
        assert!(
            matches!(
                fetch_blame_v2(ctx, repo, csid, path.clone()).await,
                Err(BlameError::NoSuchPath(_))
            ),
            "deleted {path} should be absent from the unode manifest at {csid:?}",
        );
        assert!(
            matches!(
                fetch_blame_v3(ctx, repo, csid, path.clone()).await,
                Err(BlameError::NoSuchPath(_))
            ),
            "deleted {path} should be absent from the history manifest at {csid:?}",
        );
    }

    Ok(())
}

/// The graph must actually hit the case where the history manifest reuses a
/// merge parent's file node while unodes mint a fresh merge unode — the shape
/// where the transcode writes under an id a parent also owns. If it stops
/// holding, the equivalence test below silently stops covering it.
#[mononoke::fbinit_test]
async fn test_predecessor_graph_exercises_merge_node_divergence(
    fb: FacebookInit,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = TestRepoFactory::new(fb)?.build().await?;
    borrowed!(ctx, repo);

    let graph = build_predecessor_test_graph(ctx, repo).await?;
    let c2 = graph[2].0;
    let c3 = graph[3].0;
    let path = NonRootMPath::new("dir/shared")?;

    let (_, unode_at_c2) = fetch_blame_v2(ctx, repo, c2, path.clone()).await?;
    let (_, unode_at_c3) = fetch_blame_v2(ctx, repo, c3, path.clone()).await?;
    let (_, hm_at_c2) = fetch_blame_v3(ctx, repo, c2, path.clone()).await?;
    let (_, hm_at_c3) = fetch_blame_v3(ctx, repo, c3, path.clone()).await?;

    assert_eq!(
        hm_at_c3, hm_at_c2,
        "history manifest should reuse the surviving parent's file node at the merge",
    );
    assert_ne!(
        unode_at_c3, unode_at_c2,
        "unodes should mint a fresh merge unode where the history manifest reused one",
    );

    Ok(())
}

/// Blame v3 transcoded from its predecessor must be byte-identical to blame v3
/// derived normally.
///
/// Both repos are built from the same commits, so the changeset ids — and the
/// history manifest file ids the blame hangs off — match. Comparing stored
/// blobs rather than rendered ranges is deliberate: two blames can render
/// identically while disagreeing on `max_csid_index`.
#[mononoke::fbinit_test]
async fn test_blame_v3_from_predecessor_matches_normal_derivation(
    fb: FacebookInit,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let normal_repo: TestRepo = TestRepoFactory::new(fb)?.build().await?;
    let transcode_repo: TestRepo = TestRepoFactory::new(fb)?.build().await?;
    borrowed!(ctx, normal_repo, transcode_repo);

    let normal_graph = build_predecessor_test_graph(ctx, normal_repo).await?;
    let transcode_graph = build_predecessor_test_graph(ctx, transcode_repo).await?;
    assert_eq!(
        normal_graph
            .iter()
            .map(|(csid, _)| *csid)
            .collect::<Vec<_>>(),
        transcode_graph
            .iter()
            .map(|(csid, _)| *csid)
            .collect::<Vec<_>>(),
        "both repos must contain the same commits for their blame blobs to be comparable",
    );

    let normal_blobstore = normal_repo.repo_blobstore().boxed();
    let transcode_blobstore = transcode_repo.repo_blobstore().boxed();

    for (csid, files) in &normal_graph {
        let blame_v2 = transcode_repo
            .repo_derived_data()
            .derive::<RootBlameV2>(ctx, *csid, DerivationPriority::LOW)
            .await?;
        let root_manifest = transcode_repo
            .repo_derived_data()
            .derive::<RootHistoryManifestDirectoryId>(ctx, *csid, DerivationPriority::LOW)
            .await?;
        derive_blame_v3_from_predecessor(ctx, &transcode_blobstore, blame_v2, root_manifest)
            .await?;

        for file in files {
            let path = NonRootMPath::new(file)?;
            // Derives blame v3 normally in the other repo.
            let (_, hm_file_id) = fetch_blame_v3(ctx, normal_repo, *csid, path.clone()).await?;
            let key = BlameV3Id::from(hm_file_id).blobstore_key();

            let normal = normal_blobstore
                .get(ctx, &key)
                .await?
                .ok_or_else(|| anyhow!("normal blame v3 missing for {path} at {csid:?}"))?;
            let transcoded = transcode_blobstore
                .get(ctx, &key)
                .await?
                .ok_or_else(|| anyhow!("transcoded blame v3 missing for {path} at {csid:?}"))?;

            assert_eq!(
                transcoded.into_raw_bytes(),
                normal.into_raw_bytes(),
                "transcoded blame v3 differs from normal derivation for {file} at {csid:?}",
            );
        }
    }

    Ok(())
}

fn annotate(
    content: &str,
    blame: BlameV2,
    names: &HashMap<ChangesetId, &'static str>,
) -> Result<String, Error> {
    let mut result = String::new();
    let mut ranges = blame.ranges()?;
    let mut range = ranges
        .next()
        .ok_or_else(|| Error::msg("empty blame for non empty content"))?;
    for (index, line) in content.lines().enumerate() {
        if index as u32 >= range.offset + range.length {
            range = ranges
                .next()
                .ok_or_else(|| Error::msg("not enough ranges in a blame"))?;
        }
        let name = names
            .get(&range.csid)
            .ok_or_else(|| Error::msg("unresolved csid"))?;
        result.push_str(name);
        result.push_str(": ");
        result.push_str(line);
        result.push('\n');
    }
    Ok(result)
}

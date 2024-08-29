/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use anyhow::Result;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use futures::TryStreamExt;
use itertools::Itertools;
use manifest::ManifestOps;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use pretty_assertions::assert_eq;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use tests_utils::drawdag::changes;
use tests_utils::drawdag::create_from_dag_with_changes;
use tests_utils::CreateCommitContext;

use super::*;

#[facet::container]
struct TestRepo(
    dyn BonsaiHgMapping,
    dyn Bookmarks,
    CommitGraph,
    dyn CommitGraphWriter,
    RepoDerivedData,
    RepoBlobstore,
    FilestoreConfig,
    RepoIdentity,
);

const A_FILES: &[&str] = &[
    "dir1/subdir1/subsubdir1/file1",
    "dir1/subdir1/subsubdir2/file2",
];

const B_FILES: &[&str] = &[
    "dir1/subdir1/subsubdir1/file1",
    "dir1/subdir1/subsubdir2/file2",
    "dir1/subdir1/subsubdir3/file3",
    "dir1/subdir2/subsubdir1/file1",
    "dir1/subdir2/subsubdir2/file1",
    "dir2/subdir1/subsubdir1/file1",
];

const C_FILES: &[&str] = &[
    "dir1/subdir1/subsubdir1/FILE1",
    "dir1/subdir1/SUBSUBDIR2/file2",
    "dir1/subdir1/SUBSUBDIR3/FILE3",
    "dir1/subdir2/SUBSUBDIR1/file1",
];

const D_FILES: &[&str] = &[
    "dir1/subdir1/subsubdir2/file2",
    "dir1/subdir1/SUBSUBDIR3/FILE3",
];

const E_FILES: &[&str] = &[
    "dir1/subdir1/subsubdir1/FILE1",
    "dir1/subdir2/subsubdir1/file1",
];

const F_FILES: &[&str] = &["dir1/subdir1/SUBSUBDIR3/File3"];

const G_FILES: &[&str] = &["dir3/FILE1", "dir3/file1"];

const H_FILES: &[&str] = &["dir3/file2"];

const I_FILES: &[&str] = &["dir2/subdir1/subsubdir1/FILE1"];

const J_FILES: &[&str] = &["dir2/subdir2/subsubdir1/file1"];

fn add_files<'a>(
    mut c: CreateCommitContext<'a, TestRepo>,
    files: &[&str],
) -> CreateCommitContext<'a, TestRepo> {
    for &file in files {
        c = c.add_file(file, file);
    }
    c
}

fn delete_files<'a>(
    mut c: CreateCommitContext<'a, TestRepo>,
    files: &[&str],
) -> CreateCommitContext<'a, TestRepo> {
    for &file in files {
        c = c.delete_file(file);
    }
    c
}

async fn init_repo(ctx: &CoreContext) -> Result<(TestRepo, BTreeMap<String, ChangesetId>)> {
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await.unwrap();
    let changesets = create_from_dag_with_changes(
        ctx,
        &repo,
        r"
                F-G
                 /
            A-B-C-D-E
                 \
                  H
                   \
                  I-J
        ",
        changes! {
            "A" => |c| add_files(c, A_FILES),
            "B" => |c| add_files(c, B_FILES),
            "C" => |c| add_files(c, C_FILES),
            "D" => |c| delete_files(c, D_FILES),
            "E" => |c| delete_files(c, E_FILES),
            "F" => |c| add_files(c, F_FILES),
            "G" => |c| add_files(c, G_FILES),
            "H" => |c| add_files(c, H_FILES),
            "I" => |c| add_files(c, I_FILES),
            "J" => |c| add_files(c, J_FILES),
        },
    )
    .await?;
    Ok((repo, changesets))
}

async fn assert_all_leaves(
    ctx: &CoreContext,
    repo: &TestRepo,
    cs_id: ChangesetId,
    expected: &[&str],
) -> Result<()> {
    let root_skeleton_manifest_id = repo
        .repo_derived_data()
        .derive::<RootSkeletonManifestV2Id>(ctx, cs_id)
        .await?;

    let skeleton_manifest = root_skeleton_manifest_id
        .into_inner_id()
        .load(ctx, repo.repo_blobstore())
        .await?;

    let skeleton_manifest_leaf_entries = skeleton_manifest
        .list_leaf_entries(ctx.clone(), repo.repo_blobstore_arc())
        .try_collect::<Vec<_>>()
        .await?;

    assert_eq!(
        skeleton_manifest_leaf_entries
            .into_iter()
            .map(|(path, ())| path.to_string())
            .sorted()
            .collect::<Vec<_>>(),
        expected
            .iter()
            .sorted()
            .map(|path| path.to_string())
            .collect::<Vec<_>>()
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_skeleton_manifests_v2(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    assert_all_leaves(
        &ctx,
        &repo,
        changesets["A"],
        &[
            "A",
            "dir1/subdir1/subsubdir1/file1",
            "dir1/subdir1/subsubdir2/file2",
        ],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["B"],
        &[
            "A",
            "B",
            "dir1/subdir1/subsubdir1/file1",
            "dir1/subdir1/subsubdir2/file2",
            "dir1/subdir1/subsubdir3/file3",
            "dir1/subdir2/subsubdir1/file1",
            "dir1/subdir2/subsubdir2/file1",
            "dir2/subdir1/subsubdir1/file1",
        ],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["C"],
        &[
            "A",
            "B",
            "C",
            "dir1/subdir1/SUBSUBDIR2/file2",
            "dir1/subdir1/SUBSUBDIR3/FILE3",
            "dir1/subdir1/subsubdir1/FILE1",
            "dir1/subdir1/subsubdir1/file1",
            "dir1/subdir1/subsubdir2/file2",
            "dir1/subdir1/subsubdir3/file3",
            "dir1/subdir2/SUBSUBDIR1/file1",
            "dir1/subdir2/subsubdir1/file1",
            "dir1/subdir2/subsubdir2/file1",
            "dir2/subdir1/subsubdir1/file1",
        ],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["D"],
        &[
            "A",
            "B",
            "C",
            "D",
            "dir1/subdir1/SUBSUBDIR2/file2",
            "dir1/subdir1/subsubdir1/FILE1",
            "dir1/subdir1/subsubdir1/file1",
            "dir1/subdir1/subsubdir3/file3",
            "dir1/subdir2/SUBSUBDIR1/file1",
            "dir1/subdir2/subsubdir1/file1",
            "dir1/subdir2/subsubdir2/file1",
            "dir2/subdir1/subsubdir1/file1",
        ],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["E"],
        &[
            "A",
            "B",
            "C",
            "D",
            "E",
            "dir1/subdir1/SUBSUBDIR2/file2",
            "dir1/subdir1/subsubdir1/file1",
            "dir1/subdir1/subsubdir3/file3",
            "dir1/subdir2/SUBSUBDIR1/file1",
            "dir1/subdir2/subsubdir2/file1",
            "dir2/subdir1/subsubdir1/file1",
        ],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["F"],
        &["F", "dir1/subdir1/SUBSUBDIR3/File3"],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["G"],
        &[
            "A",
            "B",
            "C",
            "F",
            "G",
            "dir1/subdir1/SUBSUBDIR2/file2",
            "dir1/subdir1/SUBSUBDIR3/FILE3",
            "dir1/subdir1/SUBSUBDIR3/File3",
            "dir1/subdir1/subsubdir1/FILE1",
            "dir1/subdir1/subsubdir1/file1",
            "dir1/subdir1/subsubdir2/file2",
            "dir1/subdir1/subsubdir3/file3",
            "dir1/subdir2/SUBSUBDIR1/file1",
            "dir1/subdir2/subsubdir1/file1",
            "dir1/subdir2/subsubdir2/file1",
            "dir2/subdir1/subsubdir1/file1",
            "dir3/FILE1",
            "dir3/file1",
        ],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["H"],
        &[
            "A",
            "B",
            "C",
            "H",
            "dir1/subdir1/SUBSUBDIR2/file2",
            "dir1/subdir1/SUBSUBDIR3/FILE3",
            "dir1/subdir1/subsubdir1/FILE1",
            "dir1/subdir1/subsubdir1/file1",
            "dir1/subdir1/subsubdir2/file2",
            "dir1/subdir1/subsubdir3/file3",
            "dir1/subdir2/SUBSUBDIR1/file1",
            "dir1/subdir2/subsubdir1/file1",
            "dir1/subdir2/subsubdir2/file1",
            "dir2/subdir1/subsubdir1/file1",
            "dir3/file2",
        ],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["I"],
        &["I", "dir2/subdir1/subsubdir1/FILE1"],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["J"],
        &[
            "A",
            "B",
            "C",
            "H",
            "I",
            "J",
            "dir1/subdir1/SUBSUBDIR2/file2",
            "dir1/subdir1/SUBSUBDIR3/FILE3",
            "dir1/subdir1/subsubdir1/FILE1",
            "dir1/subdir1/subsubdir1/file1",
            "dir1/subdir1/subsubdir2/file2",
            "dir1/subdir1/subsubdir3/file3",
            "dir1/subdir2/SUBSUBDIR1/file1",
            "dir1/subdir2/subsubdir1/file1",
            "dir1/subdir2/subsubdir2/file1",
            "dir2/subdir1/subsubdir1/FILE1",
            "dir2/subdir1/subsubdir1/file1",
            "dir2/subdir2/subsubdir1/file1",
            "dir3/file2",
        ],
    )
    .await?;

    Ok(())
}

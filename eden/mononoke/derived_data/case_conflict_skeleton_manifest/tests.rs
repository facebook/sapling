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
use skeleton_manifest_v2::RootSkeletonManifestV2Id;
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
    "DiR1/subdir1/subsubdir1/file1",
    "DiR1/subdir1/subSUBdir2/FILE2",
];

const B_FILES: &[&str] = &[
    "dir1/subdir1/subsubdir1/FIle1",
    "dir1/subdir1/subsubdir2/fILe2",
    "DIR1/subdir1/subsubDIR3/FilE3",
    "DIR1/subDIR2/subsubDIR1/FILE1",
    "Dir1/subdir2/subsubdir2/file1",
    "Dir1/subDIR1/subsubdir1/FILE1",
];

const C_FILES: &[&str] = &[
    "DIR1/subdir1/subsubdir1/FILE1",
    "Dir1/subdir1/SUBSUBDIR2/file2",
    "dIR1/SUBDIR1/SUBSUBDIR3/FILE3",
    "dir1/subDIR2/SUBsubdir1/file1",
];

const D_FILES: &[&str] = &[
    "dir1/subdir1/subsubdir2/file2",
    "dir1/subdir1/SUBSUBDIR3/FILE3",
];

const E_FILES: &[&str] = &[
    "DIR1/subdir1/SUBsubDIR1/FILE1",
    "dir1/subdir2/subsubdir1/file1",
];

const F_FILES: &[&str] = &["dir1/subdir1/SUBSUBDIR3/File3"];

const G_FILES: &[&str] = &["dir3/FILE1", "dir3/file1"];

const H_FILES: &[&str] = &["dir3/file2"];

const I_FILES: &[&str] = &["DIR2/subDIR1/subsubdir1/FILE1"];

const J_FILES: &[&str] = &["DIR2/subDIR2/subsubdir1/file1"];

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
    let root_ccsm_id = repo
        .repo_derived_data()
        .derive::<RootCaseConflictSkeletonManifestId>(ctx, cs_id)
        .await?;

    let ccsm = root_ccsm_id
        .into_inner_id()
        .load(ctx, repo.repo_blobstore())
        .await?;

    let ccsm_leaf_entries = ccsm
        .list_leaf_entries(ctx.clone(), repo.repo_blobstore_arc())
        .try_collect::<Vec<_>>()
        .await?;

    let sorted_ccsm_leaf_entries = ccsm_leaf_entries
        .into_iter()
        .map(|(path, ())| path.to_string())
        .sorted()
        .collect::<Vec<_>>();

    assert_eq!(
        sorted_ccsm_leaf_entries,
        expected
            .iter()
            .sorted()
            .map(|path| path.to_string())
            .collect::<Vec<_>>()
    );

    // Check that CCSM transformed skeleton manifest leaf entries are equivalent to CCSM leaf entries.
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

    let sorted_transformed_entries = skeleton_manifest_leaf_entries
        .into_iter()
        .flat_map(|(path, ())| Some(CcsmPath::transform(path)?.into_raw().to_string()))
        .sorted()
        .collect::<Vec<_>>();

    assert_eq!(sorted_transformed_entries, sorted_ccsm_leaf_entries);

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_case_conflict_skeleton_manifest(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    assert_all_leaves(
        &ctx,
        &repo,
        changesets["A"],
        &[
            "a/A",
            "dir1/DiR1/subdir1/subdir1/subsubdir1/subsubdir1/file1/file1",
            "dir1/DiR1/subdir1/subdir1/subsubdir2/subSUBdir2/file2/FILE2",
        ],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["B"],
        &[
            "a/A",
            "b/B",
            "dir1/DIR1/subdir1/subdir1/subsubdir3/subsubDIR3/file3/FilE3",
            "dir1/DIR1/subdir2/subDIR2/subsubdir1/subsubDIR1/file1/FILE1",
            "dir1/DiR1/subdir1/subdir1/subsubdir1/subsubdir1/file1/file1",
            "dir1/DiR1/subdir1/subdir1/subsubdir2/subSUBdir2/file2/FILE2",
            "dir1/Dir1/subdir1/subDIR1/subsubdir1/subsubdir1/file1/FILE1",
            "dir1/Dir1/subdir2/subdir2/subsubdir2/subsubdir2/file1/file1",
            "dir1/dir1/subdir1/subdir1/subsubdir1/subsubdir1/file1/FIle1",
            "dir1/dir1/subdir1/subdir1/subsubdir2/subsubdir2/file2/fILe2",
        ],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["C"],
        &[
            "a/A",
            "b/B",
            "c/C",
            "dir1/DIR1/subdir1/subdir1/subsubdir1/subsubdir1/file1/FILE1",
            "dir1/DIR1/subdir1/subdir1/subsubdir3/subsubDIR3/file3/FilE3",
            "dir1/DIR1/subdir2/subDIR2/subsubdir1/subsubDIR1/file1/FILE1",
            "dir1/DiR1/subdir1/subdir1/subsubdir1/subsubdir1/file1/file1",
            "dir1/DiR1/subdir1/subdir1/subsubdir2/subSUBdir2/file2/FILE2",
            "dir1/Dir1/subdir1/subDIR1/subsubdir1/subsubdir1/file1/FILE1",
            "dir1/Dir1/subdir1/subdir1/subsubdir2/SUBSUBDIR2/file2/file2",
            "dir1/Dir1/subdir2/subdir2/subsubdir2/subsubdir2/file1/file1",
            "dir1/dIR1/subdir1/SUBDIR1/subsubdir3/SUBSUBDIR3/file3/FILE3",
            "dir1/dir1/subdir1/subdir1/subsubdir1/subsubdir1/file1/FIle1",
            "dir1/dir1/subdir1/subdir1/subsubdir2/subsubdir2/file2/fILe2",
            "dir1/dir1/subdir2/subDIR2/subsubdir1/SUBsubdir1/file1/file1",
        ],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["D"],
        &[
            "a/A",
            "b/B",
            "c/C",
            "d/D",
            "dir1/DIR1/subdir1/subdir1/subsubdir1/subsubdir1/file1/FILE1",
            "dir1/DIR1/subdir1/subdir1/subsubdir3/subsubDIR3/file3/FilE3",
            "dir1/DIR1/subdir2/subDIR2/subsubdir1/subsubDIR1/file1/FILE1",
            "dir1/DiR1/subdir1/subdir1/subsubdir1/subsubdir1/file1/file1",
            "dir1/DiR1/subdir1/subdir1/subsubdir2/subSUBdir2/file2/FILE2",
            "dir1/Dir1/subdir1/subDIR1/subsubdir1/subsubdir1/file1/FILE1",
            "dir1/Dir1/subdir1/subdir1/subsubdir2/SUBSUBDIR2/file2/file2",
            "dir1/Dir1/subdir2/subdir2/subsubdir2/subsubdir2/file1/file1",
            "dir1/dIR1/subdir1/SUBDIR1/subsubdir3/SUBSUBDIR3/file3/FILE3",
            "dir1/dir1/subdir1/subdir1/subsubdir1/subsubdir1/file1/FIle1",
            "dir1/dir1/subdir1/subdir1/subsubdir2/subsubdir2/file2/fILe2",
            "dir1/dir1/subdir2/subDIR2/subsubdir1/SUBsubdir1/file1/file1",
        ],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["E"],
        &[
            "a/A",
            "b/B",
            "c/C",
            "d/D",
            "dir1/DIR1/subdir1/subdir1/subsubdir1/subsubdir1/file1/FILE1",
            "dir1/DIR1/subdir1/subdir1/subsubdir3/subsubDIR3/file3/FilE3",
            "dir1/DIR1/subdir2/subDIR2/subsubdir1/subsubDIR1/file1/FILE1",
            "dir1/DiR1/subdir1/subdir1/subsubdir1/subsubdir1/file1/file1",
            "dir1/DiR1/subdir1/subdir1/subsubdir2/subSUBdir2/file2/FILE2",
            "dir1/Dir1/subdir1/subDIR1/subsubdir1/subsubdir1/file1/FILE1",
            "dir1/Dir1/subdir1/subdir1/subsubdir2/SUBSUBDIR2/file2/file2",
            "dir1/Dir1/subdir2/subdir2/subsubdir2/subsubdir2/file1/file1",
            "dir1/dIR1/subdir1/SUBDIR1/subsubdir3/SUBSUBDIR3/file3/FILE3",
            "dir1/dir1/subdir1/subdir1/subsubdir1/subsubdir1/file1/FIle1",
            "dir1/dir1/subdir1/subdir1/subsubdir2/subsubdir2/file2/fILe2",
            "dir1/dir1/subdir2/subDIR2/subsubdir1/SUBsubdir1/file1/file1",
            "e/E",
        ],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["F"],
        &[
            "dir1/dir1/subdir1/subdir1/subsubdir3/SUBSUBDIR3/file3/File3",
            "f/F",
        ],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["G"],
        &[
            "a/A",
            "b/B",
            "c/C",
            "dir1/DIR1/subdir1/subdir1/subsubdir1/subsubdir1/file1/FILE1",
            "dir1/DIR1/subdir1/subdir1/subsubdir3/subsubDIR3/file3/FilE3",
            "dir1/DIR1/subdir2/subDIR2/subsubdir1/subsubDIR1/file1/FILE1",
            "dir1/DiR1/subdir1/subdir1/subsubdir1/subsubdir1/file1/file1",
            "dir1/DiR1/subdir1/subdir1/subsubdir2/subSUBdir2/file2/FILE2",
            "dir1/Dir1/subdir1/subDIR1/subsubdir1/subsubdir1/file1/FILE1",
            "dir1/Dir1/subdir1/subdir1/subsubdir2/SUBSUBDIR2/file2/file2",
            "dir1/Dir1/subdir2/subdir2/subsubdir2/subsubdir2/file1/file1",
            "dir1/dIR1/subdir1/SUBDIR1/subsubdir3/SUBSUBDIR3/file3/FILE3",
            "dir1/dir1/subdir1/subdir1/subsubdir1/subsubdir1/file1/FIle1",
            "dir1/dir1/subdir1/subdir1/subsubdir2/subsubdir2/file2/fILe2",
            "dir1/dir1/subdir1/subdir1/subsubdir3/SUBSUBDIR3/file3/File3",
            "dir1/dir1/subdir2/subDIR2/subsubdir1/SUBsubdir1/file1/file1",
            "dir3/dir3/file1/FILE1",
            "dir3/dir3/file1/file1",
            "f/F",
            "g/G",
        ],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["H"],
        &[
            "a/A",
            "b/B",
            "c/C",
            "dir1/DIR1/subdir1/subdir1/subsubdir1/subsubdir1/file1/FILE1",
            "dir1/DIR1/subdir1/subdir1/subsubdir3/subsubDIR3/file3/FilE3",
            "dir1/DIR1/subdir2/subDIR2/subsubdir1/subsubDIR1/file1/FILE1",
            "dir1/DiR1/subdir1/subdir1/subsubdir1/subsubdir1/file1/file1",
            "dir1/DiR1/subdir1/subdir1/subsubdir2/subSUBdir2/file2/FILE2",
            "dir1/Dir1/subdir1/subDIR1/subsubdir1/subsubdir1/file1/FILE1",
            "dir1/Dir1/subdir1/subdir1/subsubdir2/SUBSUBDIR2/file2/file2",
            "dir1/Dir1/subdir2/subdir2/subsubdir2/subsubdir2/file1/file1",
            "dir1/dIR1/subdir1/SUBDIR1/subsubdir3/SUBSUBDIR3/file3/FILE3",
            "dir1/dir1/subdir1/subdir1/subsubdir1/subsubdir1/file1/FIle1",
            "dir1/dir1/subdir1/subdir1/subsubdir2/subsubdir2/file2/fILe2",
            "dir1/dir1/subdir2/subDIR2/subsubdir1/SUBsubdir1/file1/file1",
            "dir3/dir3/file2/file2",
            "h/H",
        ],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["I"],
        &[
            "dir2/DIR2/subdir1/subDIR1/subsubdir1/subsubdir1/file1/FILE1",
            "i/I",
        ],
    )
    .await?;
    assert_all_leaves(
        &ctx,
        &repo,
        changesets["J"],
        &[
            "a/A",
            "b/B",
            "c/C",
            "dir1/DIR1/subdir1/subdir1/subsubdir1/subsubdir1/file1/FILE1",
            "dir1/DIR1/subdir1/subdir1/subsubdir3/subsubDIR3/file3/FilE3",
            "dir1/DIR1/subdir2/subDIR2/subsubdir1/subsubDIR1/file1/FILE1",
            "dir1/DiR1/subdir1/subdir1/subsubdir1/subsubdir1/file1/file1",
            "dir1/DiR1/subdir1/subdir1/subsubdir2/subSUBdir2/file2/FILE2",
            "dir1/Dir1/subdir1/subDIR1/subsubdir1/subsubdir1/file1/FILE1",
            "dir1/Dir1/subdir1/subdir1/subsubdir2/SUBSUBDIR2/file2/file2",
            "dir1/Dir1/subdir2/subdir2/subsubdir2/subsubdir2/file1/file1",
            "dir1/dIR1/subdir1/SUBDIR1/subsubdir3/SUBSUBDIR3/file3/FILE3",
            "dir1/dir1/subdir1/subdir1/subsubdir1/subsubdir1/file1/FIle1",
            "dir1/dir1/subdir1/subdir1/subsubdir2/subsubdir2/file2/fILe2",
            "dir1/dir1/subdir2/subDIR2/subsubdir1/SUBsubdir1/file1/file1",
            "dir2/DIR2/subdir1/subDIR1/subsubdir1/subsubdir1/file1/FILE1",
            "dir2/DIR2/subdir2/subDIR2/subsubdir1/subsubdir1/file1/file1",
            "dir3/dir3/file2/file2",
            "h/H",
            "i/I",
            "j/J",
        ],
    )
    .await?;

    Ok(())
}

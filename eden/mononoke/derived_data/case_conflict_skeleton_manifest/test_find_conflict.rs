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
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use futures::stream::FuturesUnordered;
use futures::TryStreamExt;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use mononoke_types::PrefixTrie;
use pretty_assertions::assert_eq;
use repo_blobstore::RepoBlobstore;
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
    "eden/mononoke/commit_graph",
    "eden/mononoke/derived_data",
    "eden/scm/scmquery",
    "nedn/mononoke/commit_graph",
];

const B_FILES: &[&str] = &["eden/monONoke/bonsai_hg_mapping"];

const C_FILES: &[&str] = &["eden/monONoke/bonsai_hg_mapping"];

const D_FILES: &[&str] = &["eden/mononoke/COMMIT_GRAPH", "EDEN/mononoke/derived_data"];

const E_FILES: &[&str] = &["EDEN/mononoke/derived_data"];

const F_FILES: &[&str] = &["eden/mononoke/test"];

const G_FILES: &[&str] = &["test/test/test"];

const H_FILES: &[&str] = &["nEDN/mononoke/derived_data"];

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
            D-E-F
                 \
            A-B-C-G-H
          ",
        changes! {
            "A" => |c| add_files(c, A_FILES),
            "B" => |c| add_files(c, B_FILES),
            "C" => |c| delete_files(c, C_FILES),
            "D" => |c| add_files(c, D_FILES),
            "E" => |c| delete_files(c, E_FILES),
            "F" => |c| add_files(c, F_FILES),
            "G" => |c| add_files(c, G_FILES),
            "H" => |c| add_files(c, H_FILES),
        },
    )
    .await?;
    Ok((repo, changesets))
}

async fn assert_new_case_conflict(
    ctx: &CoreContext,
    repo: &TestRepo,
    cs_id: ChangesetId,
    excluded_paths: PrefixTrie,
    expected_new_case_conflict: Option<(&str, &str)>,
) -> Result<()> {
    let root_ccsm_id = repo
        .repo_derived_data()
        .derive::<RootCaseConflictSkeletonManifestId>(ctx, cs_id)
        .await?;

    let ccsm = root_ccsm_id
        .into_inner_id()
        .load(ctx, repo.repo_blobstore())
        .await?;

    let parent_ccsms = repo
        .commit_graph()
        .changeset_parents(ctx, cs_id)
        .await?
        .into_iter()
        .map(|parent| async move {
            anyhow::Ok(
                repo.repo_derived_data()
                    .derive::<RootCaseConflictSkeletonManifestId>(ctx, parent)
                    .await?
                    .into_inner_id()
                    .load(ctx, repo.repo_blobstore())
                    .await?,
            )
        })
        .collect::<FuturesUnordered<_>>()
        .try_collect::<Vec<_>>()
        .await?;

    let new_case_conflict = ccsm
        .clone()
        .find_new_case_conflict(ctx, repo.repo_blobstore(), parent_ccsms, &excluded_paths)
        .await?
        .map(|(path1, path2)| (path1.to_string(), path2.to_string()));

    let new_case_conflict = new_case_conflict
        .as_ref()
        .map(|(path1, path2)| (path1.as_ref(), path2.as_ref()));

    assert_eq!(new_case_conflict, expected_new_case_conflict);

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_case_conflict_skeleton_manifest(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    assert_new_case_conflict(&ctx, &repo, changesets["A"], PrefixTrie::default(), None).await?;
    assert_new_case_conflict(
        &ctx,
        &repo,
        changesets["B"],
        PrefixTrie::default(),
        Some(("eden/monONoke", "eden/mononoke")),
    )
    .await?;
    assert_new_case_conflict(
        &ctx,
        &repo,
        changesets["B"],
        PrefixTrie::from_iter([MPath::new("eden")?]),
        None,
    )
    .await?;
    assert_new_case_conflict(&ctx, &repo, changesets["C"], PrefixTrie::default(), None).await?;
    assert_new_case_conflict(
        &ctx,
        &repo,
        changesets["D"],
        PrefixTrie::default(),
        Some(("EDEN", "eden")),
    )
    .await?;
    assert_new_case_conflict(
        &ctx,
        &repo,
        changesets["D"],
        PrefixTrie::from_iter([MPath::new("eden")?]),
        Some(("EDEN", "eden")),
    )
    .await?;
    assert_new_case_conflict(&ctx, &repo, changesets["D"], PrefixTrie::Included, None).await?;
    assert_new_case_conflict(&ctx, &repo, changesets["E"], PrefixTrie::default(), None).await?;
    assert_new_case_conflict(&ctx, &repo, changesets["F"], PrefixTrie::default(), None).await?;
    assert_new_case_conflict(
        &ctx,
        &repo,
        changesets["G"],
        PrefixTrie::default(),
        Some(("eden/mononoke/COMMIT_GRAPH", "eden/mononoke/commit_graph")),
    )
    .await?;
    assert_new_case_conflict(
        &ctx,
        &repo,
        changesets["G"],
        PrefixTrie::from_iter([MPath::new("eden")?]),
        None,
    )
    .await?;
    assert_new_case_conflict(
        &ctx,
        &repo,
        changesets["G"],
        PrefixTrie::from_iter([MPath::new("eden/mononoke")?]),
        None,
    )
    .await?;
    assert_new_case_conflict(
        &ctx,
        &repo,
        changesets["G"],
        PrefixTrie::from_iter([MPath::new("eden/scm")?]),
        Some(("eden/mononoke/COMMIT_GRAPH", "eden/mononoke/commit_graph")),
    )
    .await?;
    assert_new_case_conflict(
        &ctx,
        &repo,
        changesets["H"],
        PrefixTrie::default(),
        Some(("nEDN", "nedn")),
    )
    .await?;

    Ok(())
}

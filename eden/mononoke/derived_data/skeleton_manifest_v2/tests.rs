/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;

use anyhow::Result;
use blobstore::Loadable;
use blobstore::Storable;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use derivation_queue_thrift::DerivationPriority;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use futures::TryStreamExt;
use itertools::Itertools;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::ManifestParentReplacement;
use mononoke_macros::mononoke;
use mononoke_types::BlobstoreValue;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use mononoke_types::path::MPath;
use mononoke_types::skeleton_manifest_v2::SkeletonManifestV2;
use pretty_assertions::assert_eq;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use tests_utils::CreateCommitContext;
use tests_utils::drawdag::changes;
use tests_utils::drawdag::create_from_dag_with_changes;

use super::*;
use crate::derive::derive_skeleton_manifest_v2_entry;
use crate::derive::get_file_changes;
use crate::derive::inner_derive;

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
        .derive::<RootSkeletonManifestV2Id>(ctx, cs_id, DerivationPriority::LOW)
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

/// Pipeline: derive top-level dirs independently, assemble the root; must equal full derivation.
#[mononoke::fbinit_test]
async fn test_derivation_pipeline_independent_dirs(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;
    let derivation_ctx = repo.repo_derived_data().manager().derivation_context(None);
    let blobstore = repo.repo_blobstore().clone();

    let parent_csid = changesets["A"];
    let csid = changesets["B"];

    let parent_manifest = repo
        .repo_derived_data()
        .derive::<RootSkeletonManifestV2Id>(&ctx, parent_csid, DerivationPriority::LOW)
        .await?
        .into_inner_id()
        .load(&ctx, &blobstore)
        .await?;

    let expected_id = repo
        .repo_derived_data()
        .derive::<RootSkeletonManifestV2Id>(&ctx, csid, DerivationPriority::LOW)
        .await?
        .into_inner_id();

    let bcs = csid.load(&ctx, &blobstore).await?;
    let changes = get_file_changes(&bcs);

    // Derive each top-level directory subtree independently.
    let mut known_entries: HashMap<MPath, Option<Entry<SkeletonManifestV2, ()>>> = HashMap::new();
    for dir in ["dir1", "dir2"] {
        let prefix = MPath::new(dir)?;
        let parents = match parent_manifest
            .find_entry(ctx.clone(), blobstore.clone(), prefix.clone())
            .await?
        {
            Some(entry) => vec![entry],
            None => vec![],
        };
        let entry = derive_skeleton_manifest_v2_entry(
            &ctx,
            &derivation_ctx,
            parents,
            changes.clone(),
            Default::default(),
            HashMap::new(),
            prefix.clone(),
        )
        .await?;
        known_entries.insert(prefix, entry);
    }

    // Assemble the root from the staged subtrees.
    let root_entry = derive_skeleton_manifest_v2_entry(
        &ctx,
        &derivation_ctx,
        vec![Entry::Tree(parent_manifest)],
        changes,
        Default::default(),
        known_entries,
        MPath::ROOT,
    )
    .await?
    .expect("root stage should produce an entry");

    let pipeline_id = root_entry
        .into_tree()
        .expect("root entry should be a tree")
        .into_blob()
        .store(&ctx, &blobstore)
        .await?;

    // Content-addressed: equal ids imply identical trees.
    assert_eq!(pipeline_id, expected_id);

    Ok(())
}

/// Pipeline: derive a deep subtree, then its ancestor via known_entries, then the root.
#[mononoke::fbinit_test]
async fn test_derivation_pipeline_nested_dirs(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;
    let derivation_ctx = repo.repo_derived_data().manager().derivation_context(None);
    let blobstore = repo.repo_blobstore().clone();

    // Commit D's changes are all under dir1/subdir1 (plus the top-level "D" file).
    let parent_csid = changesets["C"];
    let csid = changesets["D"];

    let parent_manifest = repo
        .repo_derived_data()
        .derive::<RootSkeletonManifestV2Id>(&ctx, parent_csid, DerivationPriority::LOW)
        .await?
        .into_inner_id()
        .load(&ctx, &blobstore)
        .await?;

    let expected_id = repo
        .repo_derived_data()
        .derive::<RootSkeletonManifestV2Id>(&ctx, csid, DerivationPriority::LOW)
        .await?
        .into_inner_id();

    let bcs = csid.load(&ctx, &blobstore).await?;
    let changes = get_file_changes(&bcs);

    // Stage the deep subtree first.
    let subdir1 = MPath::new("dir1/subdir1")?;
    let subdir1_parents = match parent_manifest
        .find_entry(ctx.clone(), blobstore.clone(), subdir1.clone())
        .await?
    {
        Some(entry) => vec![entry],
        None => vec![],
    };
    let subdir1_entry = derive_skeleton_manifest_v2_entry(
        &ctx,
        &derivation_ctx,
        subdir1_parents,
        changes.clone(),
        Default::default(),
        HashMap::new(),
        subdir1.clone(),
    )
    .await?
    .expect("dir1/subdir1 stage should produce an entry");

    // Stage dir1 using the staged subdir1.
    let dir1 = MPath::new("dir1")?;
    let dir1_parents = match parent_manifest
        .find_entry(ctx.clone(), blobstore.clone(), dir1.clone())
        .await?
    {
        Some(entry) => vec![entry],
        None => vec![],
    };
    let dir1_known: HashMap<MPath, Option<Entry<SkeletonManifestV2, ()>>> =
        HashMap::from([(subdir1, Some(subdir1_entry))]);
    let dir1_entry = derive_skeleton_manifest_v2_entry(
        &ctx,
        &derivation_ctx,
        dir1_parents,
        changes.clone(),
        Default::default(),
        dir1_known,
        dir1.clone(),
    )
    .await?
    .expect("dir1 stage should produce an entry");

    // Assemble the root using the staged dir1.
    let root_known: HashMap<MPath, Option<Entry<SkeletonManifestV2, ()>>> =
        HashMap::from([(dir1, Some(dir1_entry))]);
    let root_entry = derive_skeleton_manifest_v2_entry(
        &ctx,
        &derivation_ctx,
        vec![Entry::Tree(parent_manifest)],
        changes,
        Default::default(),
        root_known,
        MPath::ROOT,
    )
    .await?
    .expect("root stage should produce an entry");

    let pipeline_id = root_entry
        .into_tree()
        .expect("root entry should be a tree")
        .into_blob()
        .store(&ctx, &blobstore)
        .await?;

    assert_eq!(pipeline_id, expected_id);

    Ok(())
}

/// Pipeline: a subtree-copy (ManifestParentReplacement) staged derivation must equal full.
#[mononoke::fbinit_test]
async fn test_derivation_pipeline_with_subtree_copy(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;
    let derivation_ctx = repo.repo_derived_data().manager().derivation_context(None);
    let blobstore = repo.repo_blobstore().clone();

    // Commit B's manifest (has both dir1 and dir2) is the parent.
    let parent_manifest = repo
        .repo_derived_data()
        .derive::<RootSkeletonManifestV2Id>(&ctx, changesets["B"], DerivationPriority::LOW)
        .await?
        .into_inner_id()
        .load(&ctx, &blobstore)
        .await?;

    // Replace dir2 with dir1's subtree.
    let dir1_entry = parent_manifest
        .find_entry(ctx.clone(), blobstore.clone(), MPath::new("dir1")?)
        .await?
        .expect("dir1 should exist in parent");
    let subtree_changes = vec![ManifestParentReplacement {
        path: MPath::new("dir2")?,
        replacements: vec![dir1_entry],
    }];

    // On top of the replacement: delete a copied file, add a new one.
    let changes: Vec<(NonRootMPath, Option<()>)> = vec![
        (NonRootMPath::new("dir2/subdir1/subsubdir1/file1")?, None),
        (NonRootMPath::new("dir2/new_file")?, Some(())),
    ];

    // Full derivation with the subtree replacement.
    let expected_id = inner_derive(
        &ctx,
        derivation_ctx.blobstore(),
        vec![parent_manifest.clone()],
        changes.clone(),
        subtree_changes.clone(),
    )
    .await?
    .expect("expected manifest")
    .into_blob()
    .store(&ctx, &blobstore)
    .await?;

    // Staged: derive dir2 with the replacement, then assemble the root.
    let dir2 = MPath::new("dir2")?;
    let dir2_parents = match parent_manifest
        .find_entry(ctx.clone(), blobstore.clone(), dir2.clone())
        .await?
    {
        Some(entry) => vec![entry],
        None => vec![],
    };
    let dir2_entry = derive_skeleton_manifest_v2_entry(
        &ctx,
        &derivation_ctx,
        dir2_parents,
        changes.clone(),
        subtree_changes.clone(),
        HashMap::new(),
        dir2.clone(),
    )
    .await?
    .expect("dir2 stage should produce an entry");

    let root_known: HashMap<MPath, Option<Entry<SkeletonManifestV2, ()>>> =
        HashMap::from([(dir2, Some(dir2_entry))]);
    let root_entry = derive_skeleton_manifest_v2_entry(
        &ctx,
        &derivation_ctx,
        vec![Entry::Tree(parent_manifest)],
        changes,
        subtree_changes,
        root_known,
        MPath::ROOT,
    )
    .await?
    .expect("root stage should produce an entry");

    let pipeline_id = root_entry
        .into_tree()
        .expect("root entry should be a tree")
        .into_blob()
        .store(&ctx, &blobstore)
        .await?;

    assert_eq!(pipeline_id, expected_id);

    Ok(())
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg(test)]
#![allow(non_snake_case)] // For test commits

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::str::FromStr;

use anyhow::Result;
use anyhow::anyhow;
use borrowed::borrowed;
use fbinit::FacebookInit;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::stream::{self};
use gitexport_tools::MASTER_BOOKMARK;
use gitexport_tools::build_partial_commit_graph_for_export;
use maplit::hashmap;
use mononoke_api::BookmarkFreshness;
use mononoke_api::BookmarkKey;
use mononoke_api::ChangesetContext;
use mononoke_api::CoreContext;
use mononoke_api::MononokeRepo;
use mononoke_api::RepoContext;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use test_utils::GitExportTestRepoOptions;
use test_utils::build_test_repo;
use test_utils::repo_with_merge_needing_ancestor_walk_on_both_sides;
use test_utils::repo_with_multiple_renamed_export_directories;
use test_utils::repo_with_octopus_merge;
use test_utils::repo_with_renamed_export_path;
use test_utils::repo_with_transparent_merge;
use test_utils::repo_with_true_merge;
use tracing::info;

#[mononoke::fbinit_test]
async fn test_partial_commit_graph_for_single_export_path(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let test_data = build_test_repo(fb, &ctx, GitExportTestRepoOptions::default()).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;

    let export_dir = NonRootMPath::new(relevant_paths["export_dir"]).unwrap();

    let A = changeset_ids["A"];
    let C = changeset_ids["C"];
    let E = changeset_ids["E"];
    let G = changeset_ids["G"];
    let I = changeset_ids["I"];
    let J = changeset_ids["J"];

    // Ids of the changesets that are expected to be rewritten
    let expected_cs_ids: Vec<ChangesetId> = vec![A, C, E, G, I, J];

    let expected_parent_map = HashMap::from([
        (A, vec![]),
        (C, vec![A]),
        (E, vec![C]),
        (G, vec![E]),
        (I, vec![G]),
        (J, vec![I]),
    ]);

    let master_cs = source_repo_ctx
        .resolve_bookmark(
            &BookmarkKey::from_str(MASTER_BOOKMARK)?,
            BookmarkFreshness::MostRecent,
        )
        .await?
        .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

    let graph_info =
        build_partial_commit_graph_for_export(vec![(export_dir, master_cs)], None).await?;

    let relevant_cs_ids = graph_info
        .changesets
        .iter()
        .map(ChangesetContext::id)
        .collect::<Vec<_>>();

    assert_eq!(expected_cs_ids, relevant_cs_ids);

    assert_eq!(expected_parent_map, graph_info.parents_map);

    Ok(())
}

/// Test that a merge commit where both branches have export-set commits
/// produces a 2-parent merge in the partial graph (instead of erroring).
///
/// DAG: A-B-C-D-E-F-G-H-I-J with K branching from E and merging into G.
/// K modifies SECOND_EXPORT_FILE. When exporting both EXPORT_DIR and
/// SECOND_EXPORT_DIR, G is a merge with export-set commits on both branches
/// (F on main, K on side).
#[mononoke::fbinit_test]
async fn test_true_merge_produces_two_parent_graph(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let test_repo_opts = GitExportTestRepoOptions {
        add_branch_commit: true,
    };

    let test_data = build_test_repo(fb, &ctx, test_repo_opts).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;

    let export_dir = NonRootMPath::new(relevant_paths["export_dir"]).unwrap();
    let second_export_dir = NonRootMPath::new(relevant_paths["second_export_dir"]).unwrap();

    let A = changeset_ids["A"];
    let C = changeset_ids["C"];
    let E = changeset_ids["E"];
    let F = changeset_ids["F"];
    let G = changeset_ids["G"];
    let K = changeset_ids["K"];

    let master_cs = source_repo_ctx
        .resolve_bookmark(
            &BookmarkKey::from_str(MASTER_BOOKMARK)?,
            BookmarkFreshness::MostRecent,
        )
        .await?
        .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

    let graph_info = build_partial_commit_graph_for_export(
        vec![
            (export_dir, master_cs.clone()),
            (second_export_dir, master_cs),
        ],
        None,
    )
    .await?;

    let relevant_cs_ids = graph_info
        .changesets
        .iter()
        .map(ChangesetContext::id)
        .collect::<Vec<_>>();

    // K has generation 6 (branches from E at gen 5), F has generation 6 too,
    // so order depends on changeset ID comparison. Both should be present.
    assert!(
        relevant_cs_ids.contains(&K),
        "K should be in the export set"
    );
    assert!(
        relevant_cs_ids.contains(&F),
        "F should be in the export set"
    );

    // G should have exactly 2 parents in the partial graph: F and K
    let g_parents = &graph_info.parents_map[&G];
    assert_eq!(
        g_parents.len(),
        2,
        "Merge commit G should have 2 parents in partial graph, got: {g_parents:?}"
    );
    assert!(
        g_parents.contains(&F),
        "G's parents should include F: {g_parents:?}"
    );
    assert!(
        g_parents.contains(&K),
        "G's parents should include K: {g_parents:?}"
    );

    // Other commits should have normal single parents
    assert_eq!(graph_info.parents_map[&A], vec![]);
    assert_eq!(graph_info.parents_map[&C], vec![A]);
    assert_eq!(graph_info.parents_map[&E], vec![C]);

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_partial_commit_graph_for_multiple_export_paths(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let test_repo_opts = GitExportTestRepoOptions {
        add_branch_commit: false,
    };

    let test_data = build_test_repo(fb, &ctx, test_repo_opts).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;

    let export_dir = NonRootMPath::new(relevant_paths["export_dir"]).unwrap();
    let second_export_dir = NonRootMPath::new(relevant_paths["second_export_dir"]).unwrap();

    let A = changeset_ids["A"];
    let C = changeset_ids["C"];
    let E = changeset_ids["E"];
    // The F commit changes only the file in the second export path
    let F = changeset_ids["F"];
    let G = changeset_ids["G"];
    let I = changeset_ids["I"];
    let J = changeset_ids["J"];

    // Ids of the changesets that are expected to be rewritten
    let expected_cs_ids: Vec<ChangesetId> = vec![A, C, E, F, G, I, J];

    let expected_parent_map = HashMap::from([
        (A, vec![]),
        (C, vec![A]),
        (E, vec![C]),
        (F, vec![E]),
        (G, vec![F]),
        (I, vec![G]),
        (J, vec![I]),
    ]);

    let master_cs = source_repo_ctx
        .resolve_bookmark(
            &BookmarkKey::from_str(MASTER_BOOKMARK)?,
            BookmarkFreshness::MostRecent,
        )
        .await?
        .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

    let graph_info = build_partial_commit_graph_for_export(
        vec![
            (export_dir, master_cs.clone()),
            (second_export_dir, master_cs),
        ],
        None,
    )
    .await?;

    let relevant_cs_ids = graph_info
        .changesets
        .iter()
        .map(ChangesetContext::id)
        .collect::<Vec<_>>();

    assert_eq!(expected_cs_ids, relevant_cs_ids);

    assert_eq!(expected_parent_map, graph_info.parents_map);

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_oldest_commit_ts_option(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let test_repo_opts = GitExportTestRepoOptions {
        add_branch_commit: false,
    };

    let test_data = build_test_repo(fb, &ctx, test_repo_opts).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;

    let export_dir = NonRootMPath::new(relevant_paths["export_dir"]).unwrap();
    let second_export_dir = NonRootMPath::new(relevant_paths["second_export_dir"]).unwrap();

    let E = changeset_ids["E"];
    // The F commit changes only the file in the second export path
    let F = changeset_ids["F"];
    let G = changeset_ids["G"];
    let I = changeset_ids["I"];
    let J = changeset_ids["J"];

    // Ids of the changesets that are expected to be rewritten
    // Ids of the changesets that are expected to be rewritten.
    // First and C commits would also be included, but we're going to
    // use E's author date as the oldest_commit_ts argument.
    let expected_cs_ids: Vec<ChangesetId> = vec![E, F, G, I, J];

    let expected_parent_map = HashMap::from([
        (E, vec![]),
        (F, vec![E]),
        (G, vec![F]),
        (I, vec![G]),
        (J, vec![I]),
    ]);

    let master_cs = source_repo_ctx
        .resolve_bookmark(
            &BookmarkKey::from_str(MASTER_BOOKMARK)?,
            BookmarkFreshness::MostRecent,
        )
        .await?
        .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

    let fifth_cs = source_repo_ctx
        .changeset(E)
        .await?
        .ok_or(anyhow!("Failed to get changeset context of E commit"))?;

    let oldest_ts = fifth_cs.author_date().await?.timestamp();

    let graph_info = build_partial_commit_graph_for_export(
        vec![
            (export_dir, master_cs.clone()),
            (second_export_dir, master_cs),
        ],
        Some(oldest_ts),
    )
    .await?;

    let relevant_cs_ids = graph_info
        .changesets
        .iter()
        .map(ChangesetContext::id)
        .collect::<Vec<_>>();

    assert_eq!(expected_cs_ids, relevant_cs_ids);

    assert_eq!(expected_parent_map, graph_info.parents_map);

    Ok(())
}

/// Test different scenarios using a history where the export path was renamed
/// throughout history.
/// The rename won't be automatically followed, but the user can provide
/// the export path's old name and the changeset where the rename happened so
/// history is followed without exporting what we don't want.
///
/// NOTE: changesets are passed as string slices and they're ids and changeset
/// contexts are fetched after the test repo is built.
async fn test_renamed_export_paths_are_followed<R: MononokeRepo>(
    source_repo_ctx: RepoContext<R>,
    changeset_ids: BTreeMap<String, ChangesetId>,
    // Path and the name of its upper bounds changeset
    export_paths: Vec<(NonRootMPath, &str)>,
    expected_relevant_changesets: Vec<&str>,
    expected_parent_map: HashMap<&str, Vec<&str>>,
) -> Result<()> {
    info!(
        "Testing renamed export paths with the following paths {0:#?}",
        export_paths
    );

    let expected_cs_ids: Vec<ChangesetId> = expected_relevant_changesets
        .into_iter()
        .map(|cs_name| changeset_ids[cs_name])
        .collect();

    let expected_parent_map: HashMap<ChangesetId, Vec<ChangesetId>> = expected_parent_map
        .into_iter()
        .map(|(cs_name, parent_names)| {
            let parent_ids: Vec<ChangesetId> =
                parent_names.into_iter().map(|p| changeset_ids[p]).collect();
            (changeset_ids[cs_name], parent_ids)
        })
        .collect();

    let export_path_infos: Vec<(NonRootMPath, ChangesetContext<R>)> = stream::iter(export_paths)
        .then(|(path, cs_name): (NonRootMPath, &str)| {
            borrowed!(changeset_ids);
            borrowed!(source_repo_ctx);
            async move {
                let cs_id = changeset_ids[cs_name];
                let cs_context = source_repo_ctx.changeset(cs_id).await?.ok_or(anyhow!(
                    "Failed to fetch changeset context from commit {cs_name}."
                ))?;

                anyhow::Ok::<(NonRootMPath, ChangesetContext<R>)>((path, cs_context))
            }
        })
        .try_collect::<Vec<(NonRootMPath, ChangesetContext<R>)>>()
        .await?;

    let graph_info = build_partial_commit_graph_for_export(export_path_infos, None).await?;

    let relevant_cs_ids = graph_info
        .changesets
        .iter()
        .map(ChangesetContext::id)
        .collect::<Vec<_>>();

    assert_eq!(expected_cs_ids, relevant_cs_ids);

    assert_eq!(expected_parent_map, graph_info.parents_map);

    Ok(())
}

/// When user manually specifies the old name of an export path along with
/// the commit where the rename happened as this paths head, the commit history
/// should be followed.
#[mononoke::fbinit_test]
async fn test_renamed_export_paths_are_followed_manually_passing_old(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let test_data = repo_with_renamed_export_path(fb, &ctx).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;

    let old_export_dir = NonRootMPath::new(relevant_paths["old_export_dir"]).unwrap();
    let new_export_dir = NonRootMPath::new(relevant_paths["new_export_dir"]).unwrap();
    let head_id: &str = test_data.head_id;

    // Passing the old name of the export path manually
    test_renamed_export_paths_are_followed(
        source_repo_ctx,
        changeset_ids,
        vec![(new_export_dir.clone(), head_id), (old_export_dir, "E")],
        vec!["A", "C", "E", "F", head_id],
        hashmap! {
            "A" => vec![],
            "C" => vec!["A"],
            "E" => vec!["C"],
            "F" => vec!["E"],
            head_id => vec!["F"],
        },
    )
    .await
}

#[mononoke::fbinit_test]
async fn test_renamed_export_paths_are_not_followed_automatically(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let test_data = repo_with_renamed_export_path(fb, &ctx).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;

    let new_export_dir = NonRootMPath::new(relevant_paths["new_export_dir"]).unwrap();
    let head_id: &str = test_data.head_id;

    test_renamed_export_paths_are_followed(
        source_repo_ctx,
        changeset_ids,
        vec![(new_export_dir.clone(), head_id)],
        vec!["E", "F", head_id],
        hashmap! {
            "E" => vec![],
            "F" => vec!["E"],
            head_id => vec!["F"],
        },
    )
    .await
}

#[mononoke::fbinit_test]
async fn test_partial_graph_with_two_renamed_export_directories(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let test_data = repo_with_multiple_renamed_export_directories(fb, &ctx).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;
    let head_id: &str = test_data.head_id;

    let new_bar = NonRootMPath::new(relevant_paths["new_bar_dir"]).unwrap();
    let new_foo = NonRootMPath::new(relevant_paths["new_foo_dir"]).unwrap();

    test_renamed_export_paths_are_followed(
        source_repo_ctx,
        changeset_ids,
        vec![(new_bar.clone(), head_id), (new_foo.clone(), head_id)],
        vec!["B", "D"], // Expected relevant commits
        hashmap! {
            "B" => vec![],
            "D" => vec!["B"],
        },
    )
    .await
}

/// Test that a merge commit where both branches have export-set commits
/// produces a 2-parent merge in the partial graph.
///
/// ```text
/// A-B-C-D
///  \ /
///   E
/// ```
///
/// - A: creates EXP/bar.txt (export path)
/// - B: deletes EXP/bar.txt (not in export set — deletions followed through)
/// - E: branches from A, modifies EXP/bar.txt
/// - C: merge of B and E, re-creates EXP/bar.txt
/// - D: modifies EXP/bar.txt
///
/// Export set: {A, E, C, D}. C's real parents [B, E] resolve to partial
/// parents [A, E] — two distinct ancestors, so C has a 2-parent merge.
#[mononoke::fbinit_test]
async fn test_true_merge_with_diamond_dag(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let test_data = repo_with_true_merge(fb, &ctx).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;

    let export_dir = NonRootMPath::new(relevant_paths["export_dir"]).unwrap();

    let A = changeset_ids["A"];
    let E = changeset_ids["E"];
    let C = changeset_ids["C"];
    let D = changeset_ids["D"];

    let master_cs = source_repo_ctx
        .resolve_bookmark(
            &BookmarkKey::from_str(MASTER_BOOKMARK)?,
            BookmarkFreshness::MostRecent,
        )
        .await?
        .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

    let graph_info =
        build_partial_commit_graph_for_export(vec![(export_dir, master_cs)], None).await?;

    let relevant_cs_ids = graph_info
        .changesets
        .iter()
        .map(ChangesetContext::id)
        .collect::<Vec<_>>();

    // Export set should be {A, E, C, D}
    assert_eq!(relevant_cs_ids.len(), 4);
    assert!(relevant_cs_ids.contains(&A));
    assert!(relevant_cs_ids.contains(&E));
    assert!(relevant_cs_ids.contains(&C));
    assert!(relevant_cs_ids.contains(&D));

    // C should have 2 parents: A and E
    let c_parents = &graph_info.parents_map[&C];
    assert_eq!(
        c_parents.len(),
        2,
        "Merge commit C should have 2 parents, got: {c_parents:?}"
    );
    assert!(c_parents.contains(&A), "C's parents should include A");
    assert!(c_parents.contains(&E), "C's parents should include E");

    // Other commits should have normal parents
    assert_eq!(graph_info.parents_map[&A], vec![]);
    assert_eq!(graph_info.parents_map[&E], vec![A]);
    assert_eq!(graph_info.parents_map[&D], vec![C]);

    Ok(())
}

/// Test that octopus merges (3+ parents) produce an N-parent merge
/// in the partial graph.
///
/// DAG:
/// ```text
///    B
///   / \
/// A---D-E
///   \ /
///    C
/// ```
///
/// All of A, B, C modify EXP/bar.txt. D is a 3-parent merge.
/// Export set: {A, B, C, D, E}. D should have 2 parents: B and C
/// (A is an ancestor of both, so it's not a direct partial parent).
#[mononoke::fbinit_test]
async fn test_octopus_merge_produces_multi_parent_graph(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let test_data = repo_with_octopus_merge(fb, &ctx).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;

    let export_dir = NonRootMPath::new(relevant_paths["export_dir"]).unwrap();

    let A = changeset_ids["A"];
    let B = changeset_ids["B"];
    let C = changeset_ids["C"];
    let D = changeset_ids["D"];
    let E = changeset_ids["E"];

    let master_cs = source_repo_ctx
        .resolve_bookmark(
            &BookmarkKey::from_str(MASTER_BOOKMARK)?,
            BookmarkFreshness::MostRecent,
        )
        .await?
        .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

    let graph_info =
        build_partial_commit_graph_for_export(vec![(export_dir, master_cs)], None).await?;

    let relevant_cs_ids = graph_info
        .changesets
        .iter()
        .map(ChangesetContext::id)
        .collect::<Vec<_>>();

    // All commits modify the export path
    assert_eq!(relevant_cs_ids.len(), 5);

    // D is an octopus merge with 3 real parents (A, B, C) in the DAG.
    // All three are in the export set, so D has 3 partial parents.
    let d_parents = &graph_info.parents_map[&D];
    assert_eq!(
        d_parents.len(),
        3,
        "Octopus merge D should have 3 parents in partial graph, got: {d_parents:?}"
    );
    assert!(d_parents.contains(&A), "D's parents should include A");
    assert!(d_parents.contains(&B), "D's parents should include B");
    assert!(d_parents.contains(&C), "D's parents should include C");

    // Other commits
    assert_eq!(graph_info.parents_map[&A], vec![]);
    assert_eq!(graph_info.parents_map[&B], vec![A]);
    assert_eq!(graph_info.parents_map[&C], vec![A]);
    assert_eq!(graph_info.parents_map[&E], vec![D]);

    Ok(())
}

/// Test that `find_nearest_export_ancestor` is exercised on BOTH parents
/// of a merge. M's real parents [c, f] are both non-export commits that
/// must walk back to different export-set ancestors (A and E).
#[mononoke::fbinit_test]
async fn test_merge_with_ancestor_walk_on_both_sides(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let test_data = repo_with_merge_needing_ancestor_walk_on_both_sides(fb, &ctx).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;

    let export_dir = NonRootMPath::new(relevant_paths["export_dir"]).unwrap();

    let A = changeset_ids["A"];
    let E = changeset_ids["E"];
    let M = changeset_ids["M"];
    let D = changeset_ids["D"];

    let master_cs = source_repo_ctx
        .resolve_bookmark(
            &BookmarkKey::from_str(MASTER_BOOKMARK)?,
            BookmarkFreshness::MostRecent,
        )
        .await?
        .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

    let graph_info =
        build_partial_commit_graph_for_export(vec![(export_dir, master_cs)], None).await?;

    let relevant_cs_ids = graph_info
        .changesets
        .iter()
        .map(ChangesetContext::id)
        .collect::<Vec<_>>();

    // Export set: {A, E, M, D} — b, c, f are skipped
    assert_eq!(relevant_cs_ids.len(), 4);
    assert!(relevant_cs_ids.contains(&A));
    assert!(relevant_cs_ids.contains(&E));
    assert!(relevant_cs_ids.contains(&M));
    assert!(relevant_cs_ids.contains(&D));

    // M's real parents [c, f] walk back to [A, E]
    let m_parents = &graph_info.parents_map[&M];
    assert_eq!(
        m_parents.len(),
        2,
        "M should have 2 partial parents after ancestor walk, got: {m_parents:?}"
    );
    assert!(m_parents.contains(&A), "M's parents should include A");
    assert!(m_parents.contains(&E), "M's parents should include E");

    // Other commits
    assert_eq!(graph_info.parents_map[&A], vec![]);
    assert_eq!(graph_info.parents_map[&E], vec![A]);
    assert_eq!(graph_info.parents_map[&D], vec![M]);

    Ok(())
}

/// Test that a merge commit is handled transparently when only one branch
/// of the merge modified the exported paths. The partial graph should
/// remain linear — the merge is invisible because the other branch has
/// no export-set commits.
///
/// DAG:
/// ```text
/// A-B-C-D
///  \ /
///   E
/// ```
///
/// Only A, C, D touch the export path. B and E only touch internal files.
/// Export set: {A, C, D}
/// C's real parents [B, E] both resolve to A in the partial graph.
/// After dedup, partial_parents = [A]. Linear!
#[mononoke::fbinit_test]
async fn test_transparent_merge_produces_linear_graph(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let test_data = repo_with_transparent_merge(fb, &ctx).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;

    let export_dir = NonRootMPath::new(relevant_paths["export_dir"]).unwrap();

    let A = changeset_ids["A"];
    let C = changeset_ids["C"];
    let D = changeset_ids["D"];

    let master_cs = source_repo_ctx
        .resolve_bookmark(
            &BookmarkKey::from_str(MASTER_BOOKMARK)?,
            BookmarkFreshness::MostRecent,
        )
        .await?
        .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

    let graph_info =
        build_partial_commit_graph_for_export(vec![(export_dir, master_cs)], None).await?;

    let relevant_cs_ids = graph_info
        .changesets
        .iter()
        .map(ChangesetContext::id)
        .collect::<Vec<_>>();

    // Only A, C, D should be in the export set (B and E only touch internal files)
    assert_eq!(relevant_cs_ids, vec![A, C, D]);

    // The partial graph should be linear: A -> C -> D
    assert_eq!(graph_info.parents_map[&A], vec![]);
    assert_eq!(graph_info.parents_map[&C], vec![A]);
    assert_eq!(graph_info.parents_map[&D], vec![C]);

    Ok(())
}

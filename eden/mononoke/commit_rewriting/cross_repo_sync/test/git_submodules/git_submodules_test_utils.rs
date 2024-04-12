/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_snake_case)]

//! Tests for handling git submodules in x-repo sync
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use ascii::AsciiString;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use bookmarks::BookmarksRef;
use bulk_derivation::BulkDerivation;
use cacheblob::InProcessLease;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use cross_repo_sync::CommitSyncRepos;
use cross_repo_sync::CommitSyncer;
use cross_repo_sync::SubmoduleDeps;
use cross_repo_sync_test_utils::rebase_root_on_master;
use cross_repo_sync_test_utils::TestRepo;
use fbinit::FacebookInit;
use fsnodes::RootFsnodeId;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use git_types::MappedGitCommitId;
use live_commit_sync_config::LiveCommitSyncConfig;
use live_commit_sync_config::TestLiveCommitSyncConfigSource;
use manifest::ManifestOps;
use maplit::btreemap;
use maplit::hashmap;
use metaconfig_types::CommitSyncConfig;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::CommonCommitSyncConfig;
use metaconfig_types::DefaultSmallToLargeCommitSyncPathAction;
use metaconfig_types::DerivedDataTypesConfig;
use metaconfig_types::GitSubmodulesChangesAction;
use metaconfig_types::RepoConfig;
use metaconfig_types::SmallRepoCommitSyncConfig;
use metaconfig_types::SmallRepoGitSubmoduleConfig;
use metaconfig_types::SmallRepoPermanentConfig;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use sorted_vector_map::SortedVectorMap;
use strum::IntoEnumIterator;
use synced_commit_mapping::SqlSyncedCommitMapping;
use test_repo_factory::TestRepoFactory;
use tests_utils::bookmark;
use tests_utils::drawdag::extend_from_dag_with_actions;
use tests_utils::CreateCommitContext;

use crate::prepare_repos_mapping_and_config;
use crate::sync_to_master;

pub const MASTER_BOOKMARK_NAME: &str = "master";

pub(crate) struct SubmoduleSyncTestData {
    pub(crate) repo_a_info: (TestRepo, BTreeMap<String, ChangesetId>),
    pub(crate) large_repo: TestRepo,
    pub(crate) commit_syncer: CommitSyncer<SqlSyncedCommitMapping, TestRepo>,
    pub(crate) live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
    pub(crate) test_sync_config_source: TestLiveCommitSyncConfigSource,
    pub(crate) mapping: SqlSyncedCommitMapping,
}

/// Simplified version of `FileChange` that allows to quickly create file change
/// information **manually** for test comparisons.
#[derive(Clone, Eq, PartialEq, Debug)]
pub(crate) enum FileChangeSummary {
    Change(FileType),
    Deletion,
}

/// All relevant information about a changeset that might be needed for test
/// assertions.
#[derive(Clone, Eq, PartialEq, Debug)]
pub(crate) struct ChangesetData {
    pub(crate) cs_id: ChangesetId,
    pub(crate) parents: Vec<ChangesetId>,
    pub(crate) message: String,
    pub(crate) file_changes: SortedVectorMap<String, FileChangeSummary>,
}

/// Helper to manually create expected changesets in a repo that can be compared
/// with the actual results
#[derive(Clone, Eq, PartialEq, Debug)]
pub(crate) struct ExpectedChangeset {
    pub(crate) message: String,
    pub(crate) file_changes: SortedVectorMap<String, FileChangeSummary>,
}

impl ExpectedChangeset {
    /// Quickly create `ExpectedChangeset` by providing a changeset message and
    /// the paths that you expect to be added/modified or deleted.
    pub(crate) fn new_by_file_change<S: Into<String> + std::cmp::Ord>(
        message: S,
        regular_changes: Vec<S>,
        deletions: Vec<S>,
    ) -> Self {
        let mut reg_changes_map = regular_changes
            .into_iter()
            .map(|p| (p.into(), FileChangeSummary::Change(FileType::Regular)))
            .collect::<SortedVectorMap<_, _>>();
        let deletions_map = deletions
            .into_iter()
            .map(|p| (p.into(), FileChangeSummary::Deletion))
            .collect::<SortedVectorMap<_, _>>();
        reg_changes_map.extend(deletions_map);

        Self {
            message: message.into(),
            file_changes: reg_changes_map,
        }
    }
}

// -----------------------------------------------------------------------------
// Builders

/// Builds the small repo (repo A) with a submodule dependency (repo B), the
/// large repo and all the commit syncer with a config that expands submodules.
pub(crate) async fn build_submodule_sync_test_data(
    fb: FacebookInit,
    repo_b: &TestRepo,
    // Add more small repo submodule dependencies for the test case
    submodule_deps: Vec<(NonRootMPath, TestRepo)>,
) -> Result<SubmoduleSyncTestData> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (small_repo, large_repo, mapping, live_commit_sync_config, test_sync_config_source) =
        prepare_repos_mapping_and_config(fb).await?;

    println!("Got small/large repos, mapping and config stores");
    let large_repo_root = CreateCommitContext::new(&ctx, &large_repo, Vec::<String>::new())
        .set_message("First commit in large repo")
        .add_files(btreemap! {"large_repo_root" => "File in large repo root"})
        .commit()
        .await?;
    let bookmark_update_ctx = bookmark(&ctx, &large_repo, MASTER_BOOKMARK_NAME);
    let _master_bookmark_key = bookmark_update_ctx.set_to(large_repo_root).await?;

    println!("Got small/large repos, mapping and config stores");
    let b_master_cs = repo_b
        .bookmarks()
        .get(ctx.clone(), &BookmarkKey::new(MASTER_BOOKMARK_NAME)?)
        .await?
        .expect("Failed to get master bookmark changeset id of repo B");
    let b_master_git_sha1 = repo_b
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, b_master_cs)
        .await?;

    let (repo_a, repo_a_cs_map) = build_repo_a(fb, small_repo, *b_master_git_sha1.oid()).await?;
    println!("Build repo_a");
    let repo_a_root = repo_a_cs_map
        .get("A_A")
        .expect("Failed to get root changeset id in repo A");

    let commit_syncer = create_repo_a_to_large_repo_commit_syncer(
        &ctx,
        repo_a.clone(),
        large_repo.clone(),
        "repo_a",
        mapping.clone(),
        live_commit_sync_config.clone(),
        test_sync_config_source.clone(),
        submodule_deps,
    )?;

    println!("Created commit syncer");

    rebase_root_on_master(ctx.clone(), &commit_syncer, *repo_a_root).await?;

    let _ = sync_to_master(
        ctx.clone(),
        &commit_syncer,
        *repo_a_cs_map.get("A_B").unwrap(),
    )
    .await?
    .ok_or(anyhow!("Failed to sync commit A_B"))?;

    let _ = sync_to_master(
        ctx.clone(),
        &commit_syncer,
        *repo_a_cs_map.get("A_C").unwrap(),
    )
    .await?
    .ok_or(anyhow!("Failed to sync commit A_C"))?;

    Ok(SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        large_repo,
        commit_syncer,
        mapping,
        live_commit_sync_config,
        test_sync_config_source,
    })
}

/// Builds repo A, which will be the small repo synced to the large repo.
/// It will depend on repo B as a submodule.
pub(crate) async fn build_repo_a(
    fb: FacebookInit,
    mut repo_a: TestRepo,
    submodule_b_git_hash: GitSha1,
) -> Result<(TestRepo, BTreeMap<String, ChangesetId>)> {
    let ctx = CoreContext::test_mock(fb);

    let available_configs = derived_data_available_config();

    let repo_config_arc = repo_a.repo_config.clone();
    let mut repo_config: RepoConfig = (*repo_config_arc).clone();
    repo_config.derived_data_config.available_configs = available_configs;

    repo_a.repo_config = Arc::new(repo_config);

    let dag = format!(
        r#"
      A_A-A_B-A_C

      # message: A_A "first commit in A"
      # message: A_B "add B submodule"
      # modify: A_B "submodules/repo_b" git-submodule &{submodule_b_git_hash}
      # message: A_C "change A after adding submodule B"
      # bookmark: A_C master
    "#
    );

    let (cs_map, _) = extend_from_dag_with_actions(&ctx, &repo_a, dag.as_str()).await?;

    Ok((repo_a, cs_map))
}

/// Builds repo B, which will be used as a submodule dependency of repo A.
pub(crate) async fn build_repo_b(
    fb: FacebookInit,
) -> Result<(TestRepo, BTreeMap<String, ChangesetId>)> {
    let ctx = CoreContext::test_mock(fb);

    const DAG: &str = r#"
      B_A-B_B

      # message: B_A "first commit in submodule B"
      # message: B_B "second commit in submodule B"
      # bookmark: B_B master
  "#;

    let repo = build_mononoke_git_mirror_repo(fb, "repo_b").await?;
    let (cs_map, _) = extend_from_dag_with_actions(&ctx, &repo, DAG).await?;

    Ok((repo, cs_map))
}

/// Builds repo B, which will be used as a submodule dependency of repo A.
pub(crate) async fn build_repo_b_with_c_submodule(
    fb: FacebookInit,
    submodule_c_git_hash: GitSha1,
    submodule_path: &NonRootMPath,
) -> Result<(TestRepo, BTreeMap<String, ChangesetId>)> {
    let ctx = CoreContext::test_mock(fb);

    let dag = format!(
        r#"
        B_A-B_B

        # message: B_A "first commit in submodule B"
        # message: B_B "second commit in submodule B"
        # modify: B_B "{submodule_path}" git-submodule &{submodule_c_git_hash}
        # bookmark: B_B master
        "#
    );

    let repo = build_mononoke_git_mirror_repo(fb, "repo_b").await?;
    let (cs_map, _) = extend_from_dag_with_actions(&ctx, &repo, dag.as_str()).await?;

    Ok((repo, cs_map))
}

/// Builds repo C, which will be used as a submodule dependency of repo A.
pub(crate) async fn build_repo_c(
    fb: FacebookInit,
) -> Result<(TestRepo, BTreeMap<String, ChangesetId>)> {
    let ctx = CoreContext::test_mock(fb);

    const DAG: &str = r#"
    C_A-C_B

    # message: C_A "[C] first commit in submodule C"
    # message: C_B "[C] second commit in submodule C"
    # bookmark: C_B master
"#;

    let repo = build_mononoke_git_mirror_repo(fb, "repo_c").await?;
    let (cs_map, _) = extend_from_dag_with_actions(&ctx, &repo, DAG).await?;

    Ok((repo, cs_map))
}

async fn build_mononoke_git_mirror_repo(fb: FacebookInit, repo_name: &str) -> Result<TestRepo> {
    let available_configs = derived_data_available_config();

    let repo = TestRepoFactory::new(fb)?
        .with_name(repo_name)
        .with_config_override(|cfg| {
            cfg.derived_data_config.available_configs = available_configs;

            // If this isn't disabled the master bookmark creation will fail
            // because skeleton manifests derivation is disabled.
            cfg.pushrebase.flags.casefolding_check = false;
        })
        .build()
        .await?;

    Ok(repo)
}

pub(crate) fn create_repo_a_to_large_repo_commit_syncer(
    ctx: &CoreContext,
    small_repo: TestRepo,
    large_repo: TestRepo,
    prefix: &str,
    mapping: SqlSyncedCommitMapping,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
    test_sync_config_source: TestLiveCommitSyncConfigSource,
    // The submodules dependency map that should be used in the commit syncer
    submodule_deps: Vec<(NonRootMPath, TestRepo)>,
) -> Result<CommitSyncer<SqlSyncedCommitMapping, TestRepo>, Error> {
    let small_repo_id = small_repo.repo_identity().id();
    let large_repo_id = large_repo.repo_identity().id();

    println!("Created commit sync config");
    let commit_sync_config =
        create_commit_sync_config(large_repo_id, small_repo_id, prefix, submodule_deps.clone())?;

    let common_config = CommonCommitSyncConfig {
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            small_repo.repo_identity().id() => SmallRepoPermanentConfig {
                bookmark_prefix: AsciiString::new(),
                common_pushrebase_bookmarks_map: HashMap::new(),
            }
        },
        large_repo_id: large_repo.repo_identity().id(),
    };

    let all_submodule_deps = submodule_deps.into_iter().collect::<HashMap<_, _>>();

    // all_submodule_deps.insert(NonRootMPath::new("submodules/repo_b")?, repo_b.clone());
    let submodule_deps = SubmoduleDeps::ForSync(all_submodule_deps);

    let repos = CommitSyncRepos::new(small_repo, large_repo, submodule_deps, &common_config)?;

    test_sync_config_source.add_config(commit_sync_config);
    test_sync_config_source.add_common_config(common_config);

    let lease = Arc::new(InProcessLease::new());
    Ok(CommitSyncer::new(
        ctx,
        mapping,
        repos,
        live_commit_sync_config,
        lease,
    ))
}

/// Creates the commit sync config to setup the sync from repo A to the large repo,
/// expanding all of its submodules.
pub(crate) fn create_commit_sync_config(
    large_repo_id: RepositoryId,
    repo_a_id: RepositoryId,
    prefix: &str,
    submodule_deps: Vec<(NonRootMPath, TestRepo)>,
) -> Result<CommitSyncConfig, Error> {
    let small_repo_config = create_small_repo_sync_config(prefix, submodule_deps)?;
    Ok(CommitSyncConfig {
        large_repo_id,
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            repo_a_id => small_repo_config,
        },
        version_name: base_commit_sync_version_name(),
    })
}

/// Creates a small repo sync config using the given submodule dependencies
pub(crate) fn create_small_repo_sync_config(
    prefix: &str,
    submodule_deps: Vec<(NonRootMPath, TestRepo)>,
) -> Result<SmallRepoCommitSyncConfig, Error> {
    let submodule_deps = submodule_deps
        .into_iter()
        .map(|(path, repo)| (path, repo.repo_identity().id()))
        .collect::<HashMap<_, _>>();

    // submodule_deps.insert(NonRootMPath::new("submodules/repo_b")?, repo_b_id);

    let small_repo_submodule_config = SmallRepoGitSubmoduleConfig {
        git_submodules_action: GitSubmodulesChangesAction::Expand,
        submodule_dependencies: submodule_deps,
        ..Default::default()
    };
    let small_repo_config = SmallRepoCommitSyncConfig {
        default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(NonRootMPath::new(
            prefix,
        )?),
        map: hashmap! {},
        submodule_config: small_repo_submodule_config,
    };
    Ok(small_repo_config)
}

pub(crate) fn add_new_commit_sync_config_version_with_submodule_deps(
    ctx: &CoreContext,
    repo_a: &TestRepo,
    large_repo: &TestRepo,
    prefix: &str,
    submodule_deps: Vec<(NonRootMPath, TestRepo)>,
    mapping: SqlSyncedCommitMapping,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
    test_sync_config_source: TestLiveCommitSyncConfigSource,
) -> Result<CommitSyncer<SqlSyncedCommitMapping, TestRepo>, Error> {
    let commit_sync_config = create_commit_sync_config(
        large_repo.repo_identity().id(),
        repo_a.repo_identity().id(),
        prefix,
        submodule_deps.clone(),
    )?;
    test_sync_config_source.add_config(commit_sync_config);
    let commit_syncer = create_repo_a_to_large_repo_commit_syncer(
        ctx,
        repo_a.clone(),
        large_repo.clone(),
        "repo_a",
        mapping.clone(),
        live_commit_sync_config.clone(),
        test_sync_config_source.clone(),
        submodule_deps,
    )?;
    Ok(commit_syncer)
}

// -----------------------------------------------------------------------------
// Test data

/// Derived data types that should be enabled in all test repos
pub(crate) fn derived_data_available_config() -> HashMap<String, DerivedDataTypesConfig> {
    let derived_data_types_config = DerivedDataTypesConfig {
        types: DerivableType::iter().collect(),
        ..Default::default()
    };

    hashmap! {
        "default".to_string() => derived_data_types_config.clone(),
        "backfilling".to_string() => derived_data_types_config
    }
}

pub(crate) fn base_commit_sync_version_name() -> CommitSyncConfigVersion {
    CommitSyncConfigVersion("TEST_VERSION_NAME".to_string())
}

pub(crate) fn expected_changesets_from_basic_setup() -> Vec<ExpectedChangeset> {
    vec![
        ExpectedChangeset::new_by_file_change(
            "First commit in large repo",
            vec!["large_repo_root"],
            vec![],
        ),
        ExpectedChangeset::new_by_file_change("first commit in A", vec!["repo_a/A_A"], vec![]),
        ExpectedChangeset::new_by_file_change(
            "add B submodule",
            vec![
                "repo_a/A_B",
                "repo_a/submodules/.x-repo-submodule-repo_b",
                "repo_a/submodules/repo_b/B_A",
                "repo_a/submodules/repo_b/B_B",
            ],
            vec![],
        ),
        ExpectedChangeset::new_by_file_change(
            "change A after adding submodule B",
            vec!["repo_a/A_C"],
            vec![],
        ),
    ]
}

// -----------------------------------------------------------------------------
// Helpers

/// Get all the relevant information from all the changesets in a repo under
/// the master bookmark.
pub(crate) async fn get_all_changeset_data_from_repo(
    ctx: &CoreContext,
    repo: &TestRepo,
) -> Result<Vec<ChangesetData>> {
    let master_cs_id = repo
        .bookmarks()
        .get(ctx.clone(), &BookmarkKey::new(MASTER_BOOKMARK_NAME)?)
        .await?
        .ok_or(anyhow!(
            "Failed to get master bookmark changeset id of repo {}",
            repo.repo_identity().name()
        ))?;

    let commit_graph = repo.commit_graph();
    let mut all_changeset_ids = commit_graph
        .ancestors_difference(ctx, vec![master_cs_id], vec![])
        .await?;

    // Order the changesets topologically
    all_changeset_ids.reverse();

    let all_changesets = stream::iter(all_changeset_ids)
        .then(|cs_id| async move {
            let bonsai = cs_id.load(ctx, repo.repo_blobstore()).await?;
            let fcs = bonsai.file_changes_map().clone();
            anyhow::Ok((bonsai, fcs))
        })
        .try_collect::<Vec<_>>()
        .await?;

    let changesets_data = all_changesets
        .into_iter()
        .map(|(bcs, fcs)| {
            let fcs_data = fcs
                .into_iter()
                .map(|(path, fc)| {
                    let fc_summary = match fc {
                        FileChange::Change(tfc) => FileChangeSummary::Change(tfc.file_type()),
                        FileChange::Deletion => FileChangeSummary::Deletion,
                        _ => panic!("Unexpected file change type"),
                    };
                    (path.to_string(), fc_summary)
                })
                .collect();

            ChangesetData {
                cs_id: bcs.get_changeset_id(),
                parents: bcs.parents().collect::<Vec<_>>().clone(),
                message: bcs.message().to_string(),
                file_changes: fcs_data,
            }
        })
        .collect();

    Ok(changesets_data)
}

/// Helper to quickly check the changesets of a specific repo considering
/// the commits from the basic setup (i.e. included in all tests)
pub(crate) fn compare_expected_changesets_from_basic_setup(
    actual_changesets: &[ChangesetData],
    expected_changesets: &[ExpectedChangeset],
) -> Result<()> {
    compare_expected_changesets(
        actual_changesets,
        expected_changesets_from_basic_setup()
            .into_iter()
            .chain(expected_changesets.iter().cloned())
            .collect::<Vec<_>>()
            .as_slice(),
    )
}

/// Helper to quickly check the changesets of a specific repo.
pub(crate) fn compare_expected_changesets(
    actual_changesets: &[ChangesetData],
    expected_changesets: &[ExpectedChangeset],
) -> Result<()> {
    // Print the actual changesets to debug test failures
    println!("actual_changesets: {:#?}\n\n", &actual_changesets);

    assert_eq!(
        actual_changesets.len(),
        expected_changesets.len(),
        "Number of expected changesets does not match actual changesets"
    );

    for (actual_changeset, expected_changeset) in
        actual_changesets.iter().zip(expected_changesets.iter())
    {
        assert_eq!(
            actual_changeset.message, expected_changeset.message,
            "Message does not match"
        );
        assert_eq!(
            actual_changeset.file_changes, expected_changeset.file_changes,
            "File changes do not match"
        );
    }
    Ok(())
}

/// Open submodule metadata file in large repo and assert that its content
/// matches the expected Git Hash (i.e. submodule pointer)
pub(crate) async fn check_submodule_metadata_file_in_large_repo<'a>(
    ctx: &'a CoreContext,
    large_repo: &'a TestRepo,
    cs_id: ChangesetId,
    metadata_file_path: NonRootMPath,
    expected_git_hash: &'a GitSha1,
) -> Result<()> {
    let fsnode_id = large_repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, cs_id)
        .await?
        .into_fsnode_id();

    let blobstore = large_repo.repo_blobstore().clone();
    let content_id = fsnode_id
        .find_entry(ctx.clone(), blobstore, metadata_file_path.clone().into())
        .await?
        .ok_or(anyhow!(
            "No fsnode entry for x-repo submodule metadata file in path {}",
            &metadata_file_path
        ))?
        .into_leaf()
        .ok_or(anyhow!("Expected metadata file fsnode entry to be a leaf"))?
        .content_id()
        .clone();
    let file_bytes = filestore::fetch_concat(large_repo.repo_blobstore(), ctx, content_id).await?;
    let file_string = std::str::from_utf8(file_bytes.as_ref())?;

    assert_eq!(file_string, expected_git_hash.to_string());

    Ok(())
}

/// Derive all the derived data types for all changesets in a repo.
/// This should be used in all tests with the large repo, to make sure that
/// commits synced to the large repo won't break the derivation of any type.
pub(crate) async fn derive_all_data_types_for_repo(
    ctx: &CoreContext,
    repo: &TestRepo,
    all_changesets: &[ChangesetData],
) -> Result<()> {
    let _ = repo
        .repo_derived_data()
        .manager()
        .derive_bulk(
            ctx,
            all_changesets.iter().map(|cs_data| cs_data.cs_id).collect(),
            None,
            DerivableType::iter().collect::<Vec<_>>().as_slice(),
        )
        .await?;

    Ok(())
}

/// Quickly check that working copy matches expectation by deriving fsnode
/// and getting the path of all leaves.
pub(crate) async fn assert_working_copy_matches_expected(
    ctx: &CoreContext,
    repo: &TestRepo,
    cs_id: ChangesetId,
    expected_files: Vec<&str>,
) -> Result<()> {
    let root_fsnode_id = repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, cs_id)
        .await?
        .into_fsnode_id();

    let blobstore = repo.repo_blobstore();
    let all_files: Vec<String> = root_fsnode_id
        .list_leaf_entries(ctx.clone(), blobstore.clone())
        .map_ok(|(path, _fsnode_file)| path.to_string())
        .try_collect()
        .await?;

    assert_eq!(
        all_files,
        expected_files
            .into_iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
        "Working copy doesn't match expectation"
    );
    Ok(())
}

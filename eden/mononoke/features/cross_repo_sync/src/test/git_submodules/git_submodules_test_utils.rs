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
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use ascii::AsciiString;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use bookmarks::BookmarksRef;
use bulk_derivation::BulkDerivation;
use commit_graph::CommitGraphRef;
use commit_transformation::SubmoduleDeps;
use context::CoreContext;
use fbinit::FacebookInit;
use fn_error_context::context;
use fsnodes::RootFsnodeId;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use git_types::MappedGitCommitId;
use justknobs::test_helpers::JustKnobsInMemory;
use justknobs::test_helpers::KnobVal;
use justknobs::test_helpers::override_just_knobs;
use live_commit_sync_config::LiveCommitSyncConfig;
use live_commit_sync_config::TestLiveCommitSyncConfigSource;
use manifest::ManifestOps;
use maplit::btreemap;
use maplit::hashmap;
use metaconfig_types::CommitSyncConfig;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::CommitSyncDirection;
use metaconfig_types::CommonCommitSyncConfig;
use metaconfig_types::DefaultSmallToLargeCommitSyncPathAction;
use metaconfig_types::DerivedDataTypesConfig;
use metaconfig_types::GitSubmodulesChangesAction;
use metaconfig_types::SmallRepoCommitSyncConfig;
use metaconfig_types::SmallRepoGitSubmoduleConfig;
use metaconfig_types::SmallRepoPermanentConfig;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use mononoke_types::hash::GitSha1;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use sorted_vector_map::SortedVectorMap;
use strum::IntoEnumIterator;
use test_repo_factory::TestRepoFactory;
use test_repo_factory::default_test_repo_derived_data_types_config;
use tests_utils::CreateCommitContext;
use tests_utils::bookmark;
use tests_utils::drawdag::extend_from_dag_with_actions;

use crate::commit_syncers_lib::CommitSyncRepos;
use crate::sync_commit::CommitSyncData;
use crate::test::prepare_repos_mapping_and_config_with_repo_config_overrides;
use crate::test::sync_to_master;
use crate::test_utils::TestRepo;
use crate::test_utils::rebase_root_on_master;

pub const MASTER_BOOKMARK_NAME: &str = "master";

pub(crate) struct SubmoduleSyncTestData {
    pub(crate) small_repo_info: (TestRepo, BTreeMap<String, ChangesetId>),
    pub(crate) large_repo_info: (TestRepo, ChangesetId),
    pub(crate) commit_sync_data: CommitSyncData<TestRepo>,
    pub(crate) live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
    pub(crate) test_sync_config_source: TestLiveCommitSyncConfigSource,
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
    pub(crate) fn new<S: Into<String> + std::cmp::Ord>(message: S) -> Self {
        Self {
            message: message.into(),
            file_changes: SortedVectorMap::new(),
        }
    }

    /// Adds Regular file changes on the given paths to ExpectedChangesets
    pub(crate) fn with_regular_changes<S, I>(self, regular_changes: I) -> Self
    where
        S: Into<String> + std::cmp::Ord,
        I: IntoIterator<Item = S>,
    {
        let reg_changes_map = regular_changes
            .into_iter()
            .map(|p| (p.into(), FileChangeSummary::Change(FileType::Regular)))
            .collect::<SortedVectorMap<_, _>>();
        self.extend_file_changes(reg_changes_map)
    }

    /// Adds Deletion file changes on the given paths to ExpectedChangesets
    pub(crate) fn with_deletions<S, I>(self, deletions: I) -> Self
    where
        S: Into<String> + std::cmp::Ord,
        I: IntoIterator<Item = S>,
    {
        let deletions_map = deletions
            .into_iter()
            .map(|p| (p.into(), FileChangeSummary::Deletion))
            .collect::<SortedVectorMap<_, _>>();
        self.extend_file_changes(deletions_map)
    }

    /// Adds GitSubmodules file changes on the given paths to ExpectedChangesets
    pub(crate) fn with_git_submodules<S, I>(self, paths: I) -> Self
    where
        S: Into<String> + std::cmp::Ord,
        I: IntoIterator<Item = S>,
    {
        let file_changes = paths
            .into_iter()
            .map(|p| (p.into(), FileChangeSummary::Change(FileType::GitSubmodule)))
            .collect::<SortedVectorMap<_, _>>();
        self.extend_file_changes(file_changes)
    }

    fn extend_file_changes(
        self,
        new_file_changes: SortedVectorMap<String, FileChangeSummary>,
    ) -> Self {
        let ExpectedChangeset {
            message,
            mut file_changes,
        } = self;

        file_changes.extend(new_file_changes);

        Self {
            message,
            file_changes,
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
    known_dangling_pointers: Vec<&str>,
) -> Result<SubmoduleSyncTestData> {
    let ctx = CoreContext::test_mock(fb.clone());
    let test_jk = JustKnobsInMemory::new(hashmap! {
        "scm/mononoke:backsync_submodule_expansion_changes".to_string() => KnobVal::Bool(true),
    });
    override_just_knobs(test_jk);

    let small_repo_ddt_cfg = submodule_repo_derived_data_types_config();

    let (small_repo, large_repo, _mapping, live_commit_sync_config, test_sync_config_source) =
        prepare_repos_mapping_and_config_with_repo_config_overrides(
            fb,
            |cfg| {
                cfg.derived_data_config.available_configs = small_repo_ddt_cfg;
            },
            |_| (),
        )
        .await?;

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
        .get(
            ctx.clone(),
            &BookmarkKey::new(MASTER_BOOKMARK_NAME)?,
            bookmarks::Freshness::MostRecent,
        )
        .await?
        .expect("Failed to get master bookmark changeset id of repo B");
    let b_master_git_sha1 = repo_b
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, b_master_cs)
        .await?;

    let (small_repo, small_repo_cs_map) =
        build_small_repo(fb, small_repo, *b_master_git_sha1.oid()).await?;
    println!("Build small_repo");
    let small_repo_root = small_repo_cs_map
        .get("A_A")
        .expect("Failed to get root changeset id in repo A");

    let commit_sync_data = create_forward_commit_syncer(
        &ctx,
        small_repo.clone(),
        large_repo.clone(),
        "small_repo",
        live_commit_sync_config.clone(),
        test_sync_config_source.clone(),
        submodule_deps,
        known_dangling_pointers,
    )?;

    println!("Created commit syncer");

    rebase_root_on_master(ctx.clone(), &commit_sync_data, *small_repo_root).await?;

    println!("Synced A_A to large repo's master");

    let _ = sync_to_master(
        ctx.clone(),
        &commit_sync_data,
        *small_repo_cs_map.get("A_B").unwrap(),
    )
    .await
    .context("Failed to sync commit A_B")?
    .ok_or(anyhow!("Commit A_B wasn't synced"))?;

    let large_repo_master = sync_to_master(
        ctx.clone(),
        &commit_sync_data,
        *small_repo_cs_map.get("A_C").unwrap(),
    )
    .await
    .context("Failed to sync commit A_C")?
    .ok_or(anyhow!("Commit A_C wasn't synced"))?;

    Ok(SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, large_repo_master),
        commit_sync_data,
        live_commit_sync_config,
        test_sync_config_source,
    })
}

/// Builds repo A, which will be the small repo synced to the large repo.
/// It will depend on repo B as a submodule.
pub(crate) async fn build_small_repo(
    fb: FacebookInit,
    small_repo: TestRepo,
    submodule_b_git_hash: GitSha1,
) -> Result<(TestRepo, BTreeMap<String, ChangesetId>)> {
    let ctx = CoreContext::test_mock(fb);

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

    let (cs_map, _) = extend_from_dag_with_actions(&ctx, &small_repo, dag.as_str()).await?;

    Ok((small_repo, cs_map))
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

    let repo = build_mononoke_git_mirror_repo(fb, "repo_b", 2).await?;
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

    let repo = build_mononoke_git_mirror_repo(fb, "repo_b", 2).await?;
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

    let repo = build_mononoke_git_mirror_repo(fb, "repo_c", 3).await?;
    let (cs_map, _) = extend_from_dag_with_actions(&ctx, &repo, DAG).await?;

    Ok((repo, cs_map))
}

async fn build_mononoke_git_mirror_repo(
    fb: FacebookInit,
    repo_name: &str,
    id: i32,
) -> Result<TestRepo> {
    let available_configs = submodule_repo_derived_data_types_config();

    let repo = TestRepoFactory::new(fb)?
        .with_name(repo_name)
        .with_id(RepositoryId::new(id))
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

pub(crate) fn create_forward_commit_syncer(
    ctx: &CoreContext,
    small_repo: TestRepo,
    large_repo: TestRepo,
    prefix: &str,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
    test_sync_config_source: TestLiveCommitSyncConfigSource,
    // The submodules dependency map that should be used in the commit syncer
    submodule_deps: Vec<(NonRootMPath, TestRepo)>,
    known_dangling_pointers: Vec<&str>,
) -> Result<CommitSyncData<TestRepo>, Error> {
    let small_repo_id = small_repo.repo_identity().id();
    let large_repo_id = large_repo.repo_identity().id();

    println!("Created commit sync config");
    let commit_sync_config = create_commit_sync_config(
        large_repo_id,
        small_repo_id,
        prefix,
        submodule_deps.clone(),
        known_dangling_pointers,
    )?;

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

    let all_submodule_deps = submodule_deps
        .into_iter()
        .map(|(p, repo)| (p, Arc::new(repo)))
        .collect::<HashMap<_, _>>();

    // all_submodule_deps.insert(NonRootMPath::new("submodules/repo_b")?, repo_b.clone());
    let submodule_deps = SubmoduleDeps::ForSync(all_submodule_deps);

    test_sync_config_source.add_config(commit_sync_config);
    test_sync_config_source.add_common_config(common_config);

    let repos = CommitSyncRepos::new(
        small_repo,
        large_repo,
        CommitSyncDirection::Forward,
        submodule_deps,
    );

    Ok(CommitSyncData::new(ctx, repos, live_commit_sync_config))
}

/// Creates the commit sync config to setup the sync from repo A to the large repo,
/// expanding all of its submodules.
pub(crate) fn create_commit_sync_config(
    large_repo_id: RepositoryId,
    small_repo_id: RepositoryId,
    prefix: &str,
    submodule_deps: Vec<(NonRootMPath, TestRepo)>,
    known_dangling_pointers: Vec<&str>,
) -> Result<CommitSyncConfig, Error> {
    let small_repo_config =
        create_small_repo_sync_config(prefix, submodule_deps, known_dangling_pointers)?;
    Ok(CommitSyncConfig {
        large_repo_id,
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            small_repo_id => small_repo_config,
        },
        version_name: base_commit_sync_version_name(),
    })
}

/// Creates a small repo sync config using the given submodule dependencies
pub(crate) fn create_small_repo_sync_config(
    prefix: &str,
    submodule_deps: Vec<(NonRootMPath, TestRepo)>,
    known_dangling_pointers: Vec<&str>,
) -> Result<SmallRepoCommitSyncConfig, Error> {
    let submodule_deps = submodule_deps
        .into_iter()
        .map(|(path, repo)| (path, repo.repo_identity().id()))
        .collect::<HashMap<_, _>>();

    // let repo_b_dangling_pointer = GitSha1::from_str(REPO_B_DANGLING_GIT_COMMIT_HASH)?;
    // let repo_c_dangling_pointer = ?;
    let dangling_submodule_pointers = known_dangling_pointers
        .clone()
        .into_iter()
        .map(|hash_str| {
            GitSha1::from_str(hash_str)
                .with_context(|| anyhow!("{hash_str} is not a valid git sha1"))
        })
        .collect::<Result<Vec<_>>>()?;

    println!("Using known dangling pointers: {known_dangling_pointers:#?}",);
    let small_repo_submodule_config = SmallRepoGitSubmoduleConfig {
        git_submodules_action: GitSubmodulesChangesAction::Expand,
        submodule_dependencies: submodule_deps,
        dangling_submodule_pointers,
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
    small_repo: &TestRepo,
    large_repo: &TestRepo,
    prefix: &str,
    submodule_deps: Vec<(NonRootMPath, TestRepo)>,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
    test_sync_config_source: TestLiveCommitSyncConfigSource,
    known_dangling_pointers: Vec<&str>,
) -> Result<CommitSyncData<TestRepo>, Error> {
    let commit_sync_config = create_commit_sync_config(
        large_repo.repo_identity().id(),
        small_repo.repo_identity().id(),
        prefix,
        submodule_deps.clone(),
        known_dangling_pointers.clone(),
    )?;
    test_sync_config_source.add_config(commit_sync_config);
    let commit_sync_data = create_forward_commit_syncer(
        ctx,
        small_repo.clone(),
        large_repo.clone(),
        "small_repo",
        live_commit_sync_config.clone(),
        test_sync_config_source.clone(),
        submodule_deps,
        known_dangling_pointers,
    )?;
    Ok(commit_sync_data)
}

// -----------------------------------------------------------------------------
// Test data

/// Derived data types that should be enabled in all test repos
pub(crate) fn submodule_repo_derived_data_types_config() -> HashMap<String, DerivedDataTypesConfig>
{
    let types = DerivableType::iter()
        .filter(|t| {
            ![
                DerivableType::HgChangesets,
                DerivableType::HgAugmentedManifests,
                DerivableType::FileNodes,
            ]
            .contains(t)
        })
        .collect();

    let default_test_repo_config = default_test_repo_derived_data_types_config();
    let derived_data_types_config = DerivedDataTypesConfig {
        types,
        ..default_test_repo_config
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
        ExpectedChangeset::new("First commit in large repo")
            .with_regular_changes(vec!["large_repo_root"]),
        ExpectedChangeset::new("first commit in A").with_regular_changes(vec!["small_repo/A_A"]),
        ExpectedChangeset::new("add B submodule").with_regular_changes(vec![
            "small_repo/A_B",
            "small_repo/submodules/.x-repo-submodule-repo_b",
            "small_repo/submodules/repo_b/B_A",
            "small_repo/submodules/repo_b/B_B",
        ]),
        ExpectedChangeset::new("change A after adding submodule B")
            .with_regular_changes(vec!["small_repo/A_C"]),
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
        .get(
            ctx.clone(),
            &BookmarkKey::new(MASTER_BOOKMARK_NAME)?,
            bookmarks::Freshness::MostRecent,
        )
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

/// Derive all enabled derived data types for all changesets in a repo.
/// This should be used in all tests with the large repo, to make sure that
/// commits synced to the large repo won't break the derivation of any type.
pub(crate) async fn derive_all_enabled_types_for_repo(
    ctx: &CoreContext,
    repo: &TestRepo,
    all_changesets: &[ChangesetData],
) -> Result<()> {
    let enabled_types = repo
        .repo_derived_data()
        .active_config()
        .types
        .iter()
        .copied()
        .collect::<Vec<_>>();

    let _ = repo
        .repo_derived_data()
        .manager()
        .derive_bulk_locally(
            ctx,
            &all_changesets
                .iter()
                .map(|cs_data| cs_data.cs_id)
                .collect::<Vec<_>>(),
            None,
            &enabled_types,
            None,
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
    println!("Asserting working copy matches expectation");
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

// Helper to easily get GitSha1 from a bonsai changeset
#[context("Failed to compute GitSha1 from changeset {cs_id} in repo {}", repo.repo_identity().name())]
pub(crate) async fn git_sha1_from_changeset(
    ctx: &CoreContext,
    repo: &TestRepo,
    cs_id: ChangesetId,
) -> Result<GitSha1> {
    let c_master_mapped_git_commit = repo
        .repo_derived_data()
        .derive::<MappedGitCommitId>(ctx, cs_id)
        .await
        .with_context(|| format!("Failed to derive MappedGitCommitId for changeset {cs_id}"))?;

    Ok(*c_master_mapped_git_commit.oid())
}

/// Sync a changeset to the target repo and derive all data types for it.
/// Also print a few things to debug test failures.
#[context("Failed to sync changeset {cs_id} to repo {}", target_repo.repo_identity().name())]
pub(crate) async fn sync_changeset_and_derive_all_types(
    ctx: CoreContext,
    cs_id: ChangesetId,
    target_repo: &TestRepo,
    commit_sync_data: &CommitSyncData<TestRepo>,
) -> Result<(ChangesetId, Vec<ChangesetData>)> {
    let target_repo_cs_id = sync_to_master(ctx.clone(), commit_sync_data, cs_id)
        .await
        .with_context(|| format!("Failed to sync commit {cs_id}"))?
        .ok_or(anyhow!("No commit was synced"))?;

    println!("Changeset {cs_id} successfully synced as {target_repo_cs_id}");

    let target_repo_changesets = get_all_changeset_data_from_repo(&ctx, target_repo).await?;

    // Print all target repo changesets for debugging, if the test fails
    println!(
        "All target repo changesets: {0:#?}",
        &target_repo_changesets
    );

    derive_all_enabled_types_for_repo(&ctx, target_repo, &target_repo_changesets).await?;

    Ok((target_repo_cs_id, target_repo_changesets.to_vec()))
}

pub(crate) async fn master_cs_id(ctx: &CoreContext, repo: &TestRepo) -> Result<ChangesetId> {
    repo.bookmarks()
        .get(
            ctx.clone(),
            &BookmarkKey::new(MASTER_BOOKMARK_NAME)?,
            bookmarks::Freshness::MostRecent,
        )
        .await?
        .ok_or(anyhow!(
            "Failed to get master bookmark changeset id of repo {}",
            repo.repo_identity().name()
        ))
}

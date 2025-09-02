/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::format_err;
use ascii::AsciiString;
use blobstore::Loadable;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateLog;
use bookmarks::BookmarkUpdateReason;
use bookmarks::Bookmarks;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use commit_transformation::SubmoduleDeps;
use commit_transformation::git_submodules::InMemoryRepo;
use commit_transformation::git_submodules::SubmoduleExpansionData;
// TODO(T182311609): stop using this directly and call cross_repo_sync methods instead
use commit_transformation::rewrite_commit;
use commit_transformation::upload_commits;
use context::CoreContext;
use filenodes::Filenodes;
use filestore::FilestoreConfig;
use git_source_of_truth::GitSourceOfTruthConfig;
use live_commit_sync_config::LiveCommitSyncConfig;
use live_commit_sync_config::TestLiveCommitSyncConfig;
use live_commit_sync_config::TestLiveCommitSyncConfigSource;
use maplit::hashmap;
use metaconfig_types::CommitSyncConfig;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::CommitSyncDirection;
use metaconfig_types::CommonCommitSyncConfig;
use metaconfig_types::DefaultSmallToLargeCommitSyncPathAction;
use metaconfig_types::RepoConfig;
use metaconfig_types::SmallRepoCommitSyncConfig;
use metaconfig_types::SmallRepoPermanentConfig;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use mutable_counters::MutableCounters;
use phases::Phases;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use repo_blobstore::RepoBlobstore;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;
use reporting::CommitSyncContext;
use sql_query_config::SqlQueryConfig;
use synced_commit_mapping::SyncedCommitMapping;
use synced_commit_mapping::SyncedCommitMappingEntry;
use test_repo_factory::TestRepoFactory;
use test_repo_factory::TestRepoFactoryBuilder;
use tests_utils::CreateCommitContext;
use tests_utils::bookmark;

use crate::commit_syncers_lib::CommitSyncRepos;
use crate::commit_syncers_lib::Syncers;
use crate::commit_syncers_lib::submodule_metadata_file_prefix_and_dangling_pointers;
use crate::commit_syncers_lib::update_mapping_with_version;
use crate::sync_commit::CommitSyncData;
use crate::sync_commit::unsafe_always_rewrite_sync_commit;
use crate::types::Repo;

#[facet::container]
#[derive(Clone)]
pub struct TestRepo {
    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    bookmark_update_log: dyn BookmarkUpdateLog,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    pushrebase_mutation_mapping: dyn PushrebaseMutationMapping,

    #[facet]
    repo_bookmark_attrs: RepoBookmarkAttrs,

    #[facet]
    filenodes: dyn Filenodes,

    #[facet]
    filestore_config: FilestoreConfig,

    #[facet]
    mutable_counters: dyn MutableCounters,

    #[facet]
    phases: dyn Phases,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    repo_cross_repo: RepoCrossRepo,

    #[facet]
    repo_config: RepoConfig,

    #[facet]
    sql_query_config: SqlQueryConfig,

    #[facet]
    git_source_of_truth_config: dyn GitSourceOfTruthConfig,
}

pub fn xrepo_mapping_version_with_small_repo() -> CommitSyncConfigVersion {
    CommitSyncConfigVersion("TEST_VERSION_NAME".to_string())
}

// Helper function that takes a root commit from source repo and rebases it on master bookmark
// in target repo
pub async fn rebase_root_on_master<R>(
    ctx: CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    source_bcs_id: ChangesetId,
) -> Result<ChangesetId, Error>
where
    R: Repo,
{
    let bookmark_name =
        BookmarkKey::new("master").context("Failed to create master bookmark key")?;
    let source_bcs = source_bcs_id
        .load(&ctx, commit_sync_data.get_source_repo().repo_blobstore())
        .await
        .context("Failed to load source bonsai")?;

    if !source_bcs.parents().collect::<Vec<_>>().is_empty() {
        return Err(format_err!("not a root commit"));
    }

    let maybe_bookmark_val = commit_sync_data
        .get_target_repo()
        .bookmarks()
        .get(
            ctx.clone(),
            &bookmark_name,
            bookmarks::Freshness::MostRecent,
        )
        .await?;

    let source_repo = commit_sync_data.get_source_repo();
    let target_repo = commit_sync_data.get_target_repo();

    let bookmark_val = maybe_bookmark_val.ok_or_else(|| format_err!("master not found"))?;
    let source_bcs_mut = source_bcs.into_mut();

    let submodule_deps = commit_sync_data.get_submodule_deps();

    let rewrite_res = {
        let map = HashMap::new();
        let version = CommitSyncConfigVersion("TEST_VERSION_NAME".to_string());
        let movers = commit_sync_data.get_movers_by_version(&version).await?;
        let (x_repo_submodule_metadata_file_prefix, dangling_submodule_pointers) =
            submodule_metadata_file_prefix_and_dangling_pointers(
                source_repo.repo_identity().id(),
                &version,
                commit_sync_data.live_commit_sync_config.clone(),
            )
            .await?;

        let small_repo = commit_sync_data.get_small_repo();
        let small_repo_id = small_repo.repo_identity().id();
        let large_repo = commit_sync_data.get_large_repo();
        let fallback_repos = vec![Arc::new(source_repo.clone())]
            .into_iter()
            .chain(submodule_deps.repos())
            .collect::<Vec<_>>();
        let large_in_memory_repo = InMemoryRepo::from_repo(large_repo, fallback_repos)?;

        let submodule_expansion_data = match submodule_deps {
            SubmoduleDeps::ForSync(deps) => Some(SubmoduleExpansionData {
                large_repo: large_in_memory_repo,
                submodule_deps: deps,
                x_repo_submodule_metadata_file_prefix: x_repo_submodule_metadata_file_prefix
                    .as_str(),
                small_repo_id,
                dangling_submodule_pointers,
            }),
            SubmoduleDeps::NotNeeded | SubmoduleDeps::NotAvailable => None,
        };

        rewrite_commit(
            &ctx,
            source_bcs_mut,
            &map,
            movers,
            source_repo,
            Default::default(),
            Default::default(),
            submodule_expansion_data,
        )
        .await?
    };
    let mut target_bcs_mut = rewrite_res.rewritten.unwrap();
    target_bcs_mut.parents = vec![bookmark_val];

    let target_bcs = target_bcs_mut.freeze()?;
    let submodule_content_ids = Vec::<(Arc<TestRepo>, HashSet<_>)>::new();

    upload_commits(
        &ctx,
        vec![target_bcs.clone()],
        commit_sync_data.get_source_repo(),
        commit_sync_data.get_target_repo(),
        submodule_content_ids,
    )
    .await?;

    let mut txn = target_repo.bookmarks().create_transaction(ctx.clone());
    txn.force_set(
        &bookmark_name,
        target_bcs.get_changeset_id(),
        BookmarkUpdateReason::TestMove,
    )
    .unwrap();
    txn.commit().await.unwrap();

    let entry = SyncedCommitMappingEntry::new(
        target_repo.repo_identity().id(),
        target_bcs.get_changeset_id(),
        source_repo.repo_identity().id(),
        source_bcs_id,
        CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
        commit_sync_data.get_source_repo_type(),
    );
    commit_sync_data.get_mapping().add(&ctx, entry).await?;

    Ok(target_bcs.get_changeset_id())
}

pub async fn init_small_large_repo<R>(
    ctx: &CoreContext,
) -> Result<
    (
        Syncers<R>,
        CommitSyncConfig,
        Arc<dyn LiveCommitSyncConfig>,
        TestLiveCommitSyncConfigSource,
    ),
    Error,
>
where
    R: Repo + for<'builder> facet::AsyncBuildable<'builder, TestRepoFactoryBuilder<'builder>>,
{
    let mut factory = TestRepoFactory::new(ctx.fb)?;
    let (sync_config, source) = TestLiveCommitSyncConfig::new_with_source();
    let sync_config = Arc::new(sync_config);
    let megarepo: R = factory
        .with_id(RepositoryId::new(1))
        .with_name("largerepo")
        .with_live_commit_sync_config(sync_config.clone())
        .build()
        .await?;
    let smallrepo: R = factory
        .with_id(RepositoryId::new(0))
        .with_name("smallrepo")
        .with_live_commit_sync_config(sync_config.clone())
        .build()
        .await?;

    let repos = CommitSyncRepos::new(
        smallrepo.clone(),
        megarepo.clone(),
        CommitSyncDirection::Forward,
        SubmoduleDeps::ForSync(HashMap::new()),
    );

    let noop_version = CommitSyncConfigVersion("noop".to_string());
    let version_with_small_repo = xrepo_mapping_version_with_small_repo();

    let noop_version_config = CommitSyncConfig {
        large_repo_id: RepositoryId::new(1),
        common_pushrebase_bookmarks: vec![BookmarkKey::new("master")?],
        small_repos: hashmap! {
            RepositoryId::new(0) => get_small_repo_sync_config_noop(),
        },
        version_name: noop_version.clone(),
    };

    let version_with_small_repo_config = CommitSyncConfig {
        large_repo_id: RepositoryId::new(1),
        common_pushrebase_bookmarks: vec![BookmarkKey::new("master")?],
        small_repos: hashmap! {
            RepositoryId::new(0) => get_small_repo_sync_config_1(),
        },
        version_name: version_with_small_repo.clone(),
    };

    source.add_config(noop_version_config);
    source.add_config(version_with_small_repo_config);

    source.add_common_config(CommonCommitSyncConfig {
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            RepositoryId::new(0) => SmallRepoPermanentConfig {
                bookmark_prefix: AsciiString::new(),
                common_pushrebase_bookmarks_map: HashMap::new(),
            }
        },
        large_repo_id: RepositoryId::new(1),
    });

    let live_commit_sync_config = sync_config.clone();

    let small_to_large_commit_syncer =
        CommitSyncData::new(ctx, repos.clone(), live_commit_sync_config.clone());

    let repos = CommitSyncRepos::new(
        smallrepo.clone(),
        megarepo.clone(),
        CommitSyncDirection::Backwards,
        SubmoduleDeps::ForSync(HashMap::new()),
    );

    let large_to_small_commit_syncer =
        CommitSyncData::new(ctx, repos.clone(), live_commit_sync_config);

    let first_bcs_id = CreateCommitContext::new_root(ctx, &smallrepo)
        .add_file("file", "content")
        .commit()
        .await?;
    let second_bcs_id = CreateCommitContext::new(ctx, &smallrepo, vec![first_bcs_id])
        .add_file("file2", "content")
        .commit()
        .await?;

    unsafe_always_rewrite_sync_commit(
        ctx,
        first_bcs_id,
        &small_to_large_commit_syncer,
        None, // parents override
        &noop_version,
        CommitSyncContext::Tests,
    )
    .await?;
    let second_large_cs_id = unsafe_always_rewrite_sync_commit(
        ctx,
        second_bcs_id,
        &small_to_large_commit_syncer,
        None, // parents override
        &noop_version,
        CommitSyncContext::Tests,
    )
    .await?
    .expect("second commit exists in large repo");

    bookmark(ctx, &smallrepo, "premove")
        .set_to(second_bcs_id)
        .await?;
    bookmark(ctx, &megarepo, "premove")
        .set_to(second_large_cs_id)
        .await?;

    bookmark(ctx, &megarepo, "megarepo_start")
        .set_to(second_large_cs_id)
        .await?;

    bookmark(ctx, &smallrepo, "megarepo_start")
        .set_to("premove")
        .await?;

    // Master commit in the small repo after "big move"
    let small_master_bcs_id = CreateCommitContext::new(ctx, &smallrepo, vec![second_bcs_id])
        .add_file("file3", "content3")
        .commit()
        .await?;

    // Master commit in large repo after "big move"
    let large_master_bcs_id = CreateCommitContext::new(ctx, &megarepo, vec![second_large_cs_id])
        .add_file("prefix/file3", "content3")
        .commit()
        .await?;

    bookmark(ctx, &smallrepo, "master")
        .set_to(small_master_bcs_id)
        .await?;
    bookmark(ctx, &megarepo, "master")
        .set_to(large_master_bcs_id)
        .await?;

    update_mapping_with_version(
        ctx,
        hashmap! { small_master_bcs_id => large_master_bcs_id},
        &small_to_large_commit_syncer,
        &version_with_small_repo,
    )
    .await?;

    println!(
        "small master: {}, large master: {}",
        small_master_bcs_id, large_master_bcs_id
    );
    println!(
        "{:?}",
        small_to_large_commit_syncer
            .get_commit_sync_outcome(ctx, small_master_bcs_id)
            .await?
    );

    Ok((
        Syncers {
            small_to_large: small_to_large_commit_syncer,
            large_to_small: large_to_small_commit_syncer,
        },
        base_commit_sync_config(&megarepo, &smallrepo),
        sync_config,
        source,
    ))
}

pub fn base_commit_sync_config(
    large_repo: &impl RepoIdentityRef,
    small_repo: &impl RepoIdentityRef,
) -> CommitSyncConfig {
    let small_repo_sync_config = SmallRepoCommitSyncConfig {
        default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(
            NonRootMPath::new("prefix").unwrap(),
        ),
        map: hashmap! {},
        submodule_config: Default::default(),
    };
    CommitSyncConfig {
        large_repo_id: large_repo.repo_identity().id(),
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            small_repo.repo_identity().id() => small_repo_sync_config,
        },
        version_name: CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
    }
}

pub fn get_live_commit_sync_config() -> Arc<dyn LiveCommitSyncConfig> {
    let (sync_config, source) = TestLiveCommitSyncConfig::new_with_source();

    let bookmark_prefix = AsciiString::from_str("small").unwrap();
    let first_version = CommitSyncConfig {
        large_repo_id: RepositoryId::new(0),
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            RepositoryId::new(1) => get_small_repo_sync_config_1(),
        },
        version_name: CommitSyncConfigVersion("first_version".to_string()),
    };

    let second_version = CommitSyncConfig {
        large_repo_id: RepositoryId::new(0),
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            RepositoryId::new(1) => get_small_repo_sync_config_2(),
        },
        version_name: CommitSyncConfigVersion("second_version".to_string()),
    };

    source.add_config(first_version);
    source.add_config(second_version);

    source.add_common_config(CommonCommitSyncConfig {
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            RepositoryId::new(1) => SmallRepoPermanentConfig {
                bookmark_prefix,
                common_pushrebase_bookmarks_map: HashMap::new(),
            }
        },
        large_repo_id: RepositoryId::new(0),
    });

    Arc::new(sync_config)
}

fn get_small_repo_sync_config_noop() -> SmallRepoCommitSyncConfig {
    SmallRepoCommitSyncConfig {
        default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(
            NonRootMPath::new("prefix").unwrap(),
        ),
        map: hashmap! {},
        submodule_config: Default::default(),
    }
}

fn get_small_repo_sync_config_1() -> SmallRepoCommitSyncConfig {
    SmallRepoCommitSyncConfig {
        default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(
            NonRootMPath::new("prefix").unwrap(),
        ),
        map: hashmap! {},
        submodule_config: Default::default(),
    }
}

fn get_small_repo_sync_config_2() -> SmallRepoCommitSyncConfig {
    SmallRepoCommitSyncConfig {
        default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(
            NonRootMPath::new("prefix").unwrap(),
        ),
        map: hashmap! {
            NonRootMPath::new("special").unwrap() => NonRootMPath::new("special").unwrap(),
        },
        submodule_config: Default::default(),
    }
}

// TODO(T168676855): define small repo config that strips submodules and add tests

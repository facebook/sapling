/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use mercurial_derivation::derive_hg_changeset::DeriveHgChangeset;
use metaconfig_types::RepoConfig;
use metaconfig_types::RestrictedPathsConfig;
use mononoke_macros::mononoke;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use permission_checker::MononokeIdentity;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use restricted_paths::SqlRestrictedPathsManifestIdStoreBuilder;
use restricted_paths::*;
use sql_construct::SqlConstruct;
use test_repo_factory::TestRepoFactory;
use tests_utils::CreateCommitContext;

#[facet::container]
pub struct TestRepo {
    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    repo_config: RepoConfig,

    #[facet]
    restricted_paths: RestrictedPaths,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    filestore_config: FilestoreConfig,

    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,
}

#[mononoke::fbinit_test]
async fn test_mercurial_manifest_no_restricted_change(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let all_entries = hg_manifest_test_with_restricted_paths(
        &ctx,
        vec![(
            NonRootMPath::new("restricted/dir").unwrap(),
            MononokeIdentity::from_str("SERVICE_IDENTITY:restricted_acl")?,
        )],
        vec!["unrestricted/dir/a"],
    )
    .await?;

    assert!(all_entries.is_empty(), "Manifest id store should be empty");

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_mercurial_manifest_single_dir_single_restricted_change(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let all_entries = hg_manifest_test_with_restricted_paths(
        &ctx,
        vec![(
            NonRootMPath::new("restricted/dir").unwrap(),
            MononokeIdentity::from_str("SERVICE_IDENTITY:restricted_acl")?,
        )],
        vec!["restricted/dir/a"],
    )
    .await?;
    assert!(
        !all_entries.is_empty(),
        "Expected restricted paths manifest ids to be stored"
    );

    pretty_assertions::assert_eq!(
        all_entries,
        vec![(ManifestType::Hg, "restricted/dir".to_string())]
    );

    Ok(())
}

// Multiple files in a single restricted directory generate a single entry in
// the manifest id store.
#[mononoke::fbinit_test]
async fn test_mercurial_manifest_single_dir_many_restricted_changes(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let all_entries = hg_manifest_test_with_restricted_paths(
        &ctx,
        vec![(
            NonRootMPath::new("restricted/dir").unwrap(),
            MononokeIdentity::from_str("SERVICE_IDENTITY:restricted_acl")?,
        )],
        vec!["restricted/dir/a", "restricted/dir/b"],
    )
    .await?;
    assert!(
        !all_entries.is_empty(),
        "Expected restricted paths manifest ids to be stored"
    );

    pretty_assertions::assert_eq!(
        all_entries,
        vec![(ManifestType::Hg, "restricted/dir".to_string())]
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_mercurial_manifest_single_dir_restricted_and_unrestricted(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let all_entries = hg_manifest_test_with_restricted_paths(
        &ctx,
        vec![(
            NonRootMPath::new("restricted/dir").unwrap(),
            MononokeIdentity::from_str("SERVICE_IDENTITY:restricted_acl")?,
        )],
        vec!["restricted/dir/a", "unrestricted/dir/b"],
    )
    .await?;
    assert!(
        !all_entries.is_empty(),
        "Expected restricted paths manifest ids to be stored"
    );

    pretty_assertions::assert_eq!(
        all_entries,
        vec![(ManifestType::Hg, "restricted/dir".to_string())]
    );

    Ok(())
}

// Multiple restricted directories generate multiple entries in the manifest
#[mononoke::fbinit_test]
async fn test_mercurial_manifest_multiple_restricted_dirs(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let all_entries = hg_manifest_test_with_restricted_paths(
        &ctx,
        vec![
            (
                NonRootMPath::new("restricted/one").unwrap(),
                MononokeIdentity::from_str("SERVICE_IDENTITY:restricted_acl")?,
            ),
            (
                NonRootMPath::new("restricted/two").unwrap(),
                MononokeIdentity::from_str("SERVICE_IDENTITY:another_acl")?,
            ),
        ],
        vec!["restricted/one/a", "restricted/two/b"],
    )
    .await?;

    assert!(
        !all_entries.is_empty(),
        "Expected restricted paths manifest ids to be stored"
    );

    pretty_assertions::assert_eq!(
        all_entries,
        vec![
            (ManifestType::Hg, "restricted/one".to_string()),
            (ManifestType::Hg, "restricted/two".to_string()),
        ]
    );

    Ok(())
}

// TODO(T239041722): test overlapping restricted directories. Top-level ACL should
// be enforced.

//
// ----------------------------------------------------------------
// Test helpers

/// Given a list of restricted paths and a list of file paths, create a changeset
/// modifying those paths, derive the hg manifest and return all the entries
/// in the manifest id store.
async fn hg_manifest_test_with_restricted_paths(
    ctx: &CoreContext,
    restricted_paths: Vec<(NonRootMPath, MononokeIdentity)>,
    file_paths: Vec<&str>,
) -> Result<Vec<(ManifestType, String)>> {
    let repo = setup_test_repo(ctx, restricted_paths).await?;
    let mut commit_ctx = CreateCommitContext::new_root(ctx, &repo);
    for path in file_paths {
        commit_ctx = commit_ctx.add_file(path, path.to_string());
    }

    let bcs_id = commit_ctx.commit().await?;

    // Get the hg changeset id for the commit, to trigger hg manifest derivation
    let _hg_cs_id = repo.derive_hg_changeset(ctx, bcs_id).await?;

    let all_entries = repo
        .restricted_paths()
        .manifest_id_store()
        .get_all_entries(ctx)
        .await?;

    println!("{:?}", all_entries);

    Ok(all_entries
        .into_iter()
        .map(|e| (e.manifest_type, e.path.to_string()))
        .collect())
}
async fn setup_test_repo(
    ctx: &CoreContext,
    restricted_paths: Vec<(NonRootMPath, MononokeIdentity)>,
) -> Result<TestRepo> {
    let repo_id = RepositoryId::new(0);

    let path_acls = restricted_paths.into_iter().collect();

    let manifest_id_store = Arc::new(
        SqlRestrictedPathsManifestIdStoreBuilder::with_sqlite_in_memory()
            .expect("Failed to create Sqlite connection")
            .with_repo_id(repo_id),
    );

    let config = RestrictedPathsConfig { path_acls };
    let repo_restricted_paths = Arc::new(RestrictedPaths::new(config, manifest_id_store));

    // Create the test repo
    let mut factory = TestRepoFactory::new(ctx.fb)?;
    let repo = factory
        .with_restricted_paths(repo_restricted_paths.clone())
        .build()
        .await?;
    Ok(repo)
}

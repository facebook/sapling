/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::Result;
use fbinit::FacebookInit;
use mononoke_macros::mononoke;
use mononoke_types::NonRootMPath;
use mononoke_types::RepoPath;
use mononoke_types::RepositoryId;
use permission_checker::MononokeIdentity;
use restricted_paths::*;

mod utils;
use utils::*;

#[mononoke::fbinit_test]
async fn test_no_restricted_change(fb: FacebookInit) -> Result<()> {
    let restricted_paths = vec![(
        NonRootMPath::new("restricted/dir").unwrap(),
        MononokeIdentity::from_str("REPO_REGION:restricted_acl")?,
    )];
    RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![("unrestricted/dir/a", None)])
        .expecting_manifest_id_store_entries(vec![])
        .expecting_scuba_access_logs(vec![])
        .build(fb)
        .await?
        .run_restricted_paths_test()
        .await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_change_to_restricted_with_access_is_logged(fb: FacebookInit) -> Result<()> {
    let project_acl = MononokeIdentity::from_str("REPO_REGION:myusername_project")?;
    let restricted_paths = vec![(
        NonRootMPath::new("user_project/foo").unwrap(),
        project_acl.clone(),
    )];

    let expected_manifest_id = ManifestId::from("f15543536ef8c0578589b6aa5a85e49233f38a6b");

    RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![("user_project/foo/bar/a", None)])
        .expecting_manifest_id_store_entries(vec![
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_manifest_id.clone(),
                RepoPath::dir("user_project/foo")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::HgAugmented,
                expected_manifest_id.clone(),
                RepoPath::dir("user_project/foo")?,
            )?,
        ])
        .expecting_scuba_access_logs(vec![
            // HgManifest access log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                // The restricted path root is logged, not the full path
                restricted_paths: cast_to_non_root_mpaths(vec!["user_project/foo"]),
                manifest_id: Some(expected_manifest_id.clone()),
                manifest_type: Some(ManifestType::Hg),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: true,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![project_acl.clone()],
            },
            // HgAugmentedManifest access log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                // The restricted path root is logged, not the full path
                restricted_paths: cast_to_non_root_mpaths(vec!["user_project/foo"]),
                manifest_id: Some(expected_manifest_id.clone()),
                manifest_type: Some(ManifestType::HgAugmented),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: true,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![project_acl.clone()],
            },
            // Path access logs for directories traversed
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["user_project/foo"]),
                manifest_id: None,
                manifest_type: None,
                full_path: Some(NonRootMPath::new("user_project/foo")?),
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: true,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![project_acl.clone()],
            },
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["user_project/foo"]),
                manifest_id: None,
                manifest_type: None,
                full_path: Some(NonRootMPath::new("user_project/foo/bar")?),
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: true,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![project_acl],
            },
        ])
        .build(fb)
        .await?
        .run_restricted_paths_test()
        .await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_single_dir_single_restricted_change(fb: FacebookInit) -> Result<()> {
    let restricted_acl = MononokeIdentity::from_str("REPO_REGION:restricted_acl")?;
    let restricted_paths = vec![(
        NonRootMPath::new("restricted/dir").unwrap(),
        restricted_acl.clone(),
    )];

    let expected_manifest_id = ManifestId::from("0e3837eaab4fb0454c78f290aeb747a201ccd05b");

    RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![("restricted/dir/a", None)])
        .expecting_manifest_id_store_entries(vec![
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_manifest_id.clone(),
                RepoPath::dir("restricted/dir")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::HgAugmented,
                expected_manifest_id.clone(),
                RepoPath::dir("restricted/dir")?,
            )?,
        ])
        .expecting_scuba_access_logs(vec![
            // HgManifest access log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/dir"]),
                manifest_id: Some(expected_manifest_id.clone()),
                manifest_type: Some(ManifestType::Hg),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
            // HgAugmentedManifest access log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/dir"]),
                manifest_id: Some(expected_manifest_id.clone()),
                manifest_type: Some(ManifestType::HgAugmented),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
            // Path access log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/dir"]),
                manifest_id: None,
                manifest_type: None,
                full_path: Some(NonRootMPath::new("restricted/dir")?),
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
        ])
        .build(fb)
        .await?
        .run_restricted_paths_test()
        .await?;

    Ok(())
}

// Multiple files in a single restricted directory generate a single entry in
// the manifest id store.
#[mononoke::fbinit_test]
async fn test_single_dir_many_restricted_changes(fb: FacebookInit) -> Result<()> {
    let restricted_acl = MononokeIdentity::from_str("REPO_REGION:restricted_acl")?;
    let restricted_paths = vec![(
        NonRootMPath::new("restricted/dir").unwrap(),
        restricted_acl.clone(),
    )];

    let expected_manifest_id = ManifestId::from("3132e75d8439632fc89f193cbf4f02b2b5428c6e");

    RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![("restricted/dir/a", None), ("restricted/dir/b", None)])
        .expecting_manifest_id_store_entries(vec![
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_manifest_id.clone(),
                RepoPath::dir("restricted/dir")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::HgAugmented,
                expected_manifest_id.clone(),
                RepoPath::dir("restricted/dir")?,
            )?,
        ])
        .expecting_scuba_access_logs(vec![
            // HgManifest access log - Single log entry for both files, because they're under the same
            // restricted directory
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/dir"]),
                manifest_id: Some(expected_manifest_id.clone()),
                manifest_type: Some(ManifestType::Hg),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
            // HgAugmentedManifest access log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/dir"]),
                manifest_id: Some(expected_manifest_id.clone()),
                manifest_type: Some(ManifestType::HgAugmented),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
            // Path access log - for the directory containing both files
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/dir"]),
                manifest_id: None,
                manifest_type: None,
                full_path: Some(NonRootMPath::new("restricted/dir")?),
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
        ])
        .build(fb)
        .await?
        .run_restricted_paths_test()
        .await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_single_dir_restricted_and_unrestricted(fb: FacebookInit) -> Result<()> {
    let restricted_acl = MononokeIdentity::from_str("REPO_REGION:restricted_acl")?;
    let restricted_paths = vec![(
        NonRootMPath::new("restricted/dir").unwrap(),
        restricted_acl.clone(),
    )];

    let expected_manifest_id = ManifestId::from("0e3837eaab4fb0454c78f290aeb747a201ccd05b");

    RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![
            ("restricted/dir/a", None),
            ("unrestricted/dir/b", None),
        ])
        .expecting_manifest_id_store_entries(vec![
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_manifest_id.clone(),
                RepoPath::dir("restricted/dir")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::HgAugmented,
                expected_manifest_id.clone(),
                RepoPath::dir("restricted/dir")?,
            )?,
        ])
        .expecting_scuba_access_logs(vec![
            // HgManifest access log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/dir"]),
                manifest_id: Some(expected_manifest_id.clone()),
                manifest_type: Some(ManifestType::Hg),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
            // HgAugmentedManifest access log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/dir"]),
                manifest_id: Some(expected_manifest_id.clone()),
                manifest_type: Some(ManifestType::HgAugmented),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
            // Path access log - only for restricted directory
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/dir"]),
                manifest_id: None,
                manifest_type: None,
                full_path: Some(NonRootMPath::new("restricted/dir")?),
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
        ])
        .build(fb)
        .await?
        .run_restricted_paths_test()
        .await?;

    Ok(())
}

// Multiple restricted directories generate multiple entries in the manifest
#[mononoke::fbinit_test]
async fn test_multiple_restricted_dirs(fb: FacebookInit) -> Result<()> {
    let restricted_acl = MononokeIdentity::from_str("REPO_REGION:restricted_acl")?;
    let another_acl = MononokeIdentity::from_str("REPO_REGION:another_acl")?;
    let restricted_paths = vec![
        (
            NonRootMPath::new("restricted/one").unwrap(),
            restricted_acl.clone(),
        ),
        (
            NonRootMPath::new("restricted/two").unwrap(),
            another_acl.clone(),
        ),
    ];

    let expected_hg_manifest_id_one = ManifestId::from("e53be16502cbc6afeb30ef30de7f6d9841fd4cb1");
    let expected_hg_manifest_id_two = ManifestId::from("f5ca206223b4d531f0d65ff422273f901bc7a024");

    RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![("restricted/one/a", None), ("restricted/two/b", None)])
        .expecting_manifest_id_store_entries(vec![
            RestrictedPathManifestIdEntry::new(
                ManifestType::HgAugmented,
                expected_hg_manifest_id_two.clone(),
                RepoPath::dir("restricted/two")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::HgAugmented,
                expected_hg_manifest_id_one.clone(),
                RepoPath::dir("restricted/one")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_hg_manifest_id_one.clone(),
                RepoPath::dir("restricted/one")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_hg_manifest_id_two.clone(),
                RepoPath::dir("restricted/two")?,
            )?,
        ])
        .expecting_scuba_access_logs(vec![
            // restricted/two access - HgManifest log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/two"]),
                manifest_id: Some(expected_hg_manifest_id_two.clone()),
                manifest_type: Some(ManifestType::Hg),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![another_acl.clone()],
            },
            // restricted/two access - HgAugmentedManifest log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/two"]),
                manifest_id: Some(expected_hg_manifest_id_two.clone()),
                manifest_type: Some(ManifestType::HgAugmented),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![another_acl.clone()],
            },
            // restricted/two access - path log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/two"]),
                manifest_id: None,
                manifest_type: None,
                full_path: Some(NonRootMPath::new("restricted/two")?),
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![another_acl.clone()],
            },
            // restricted/one access - HgManifest log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/one"]),
                manifest_id: Some(expected_hg_manifest_id_one.clone()),
                manifest_type: Some(ManifestType::Hg),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
            // restricted/one access - HgAugmentedManifest log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/one"]),
                manifest_id: Some(expected_hg_manifest_id_one.clone()),
                manifest_type: Some(ManifestType::HgAugmented),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
            // restricted/one access - path log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/one"]),
                manifest_id: None,
                manifest_type: None,
                full_path: Some(NonRootMPath::new("restricted/one")?),
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
        ])
        .build(fb)
        .await?
        .run_restricted_paths_test()
        .await?;

    Ok(())
}

// Test that if the user has access to one of the restricted paths, there will
// be a log entry for each one with the proper authorization result.
#[mononoke::fbinit_test]
async fn test_multiple_restricted_dirs_with_partial_access(fb: FacebookInit) -> Result<()> {
    let restricted_acl = MononokeIdentity::from_str("REPO_REGION:restricted_acl")?;
    let myusername_project_acl = MononokeIdentity::from_str("REPO_REGION:myusername_project")?;
    let restricted_paths = vec![
        (
            NonRootMPath::new("restricted/one").unwrap(),
            restricted_acl.clone(),
        ),
        (
            // User will have access to this path
            NonRootMPath::new("user_project/foo").unwrap(),
            myusername_project_acl.clone(),
        ),
    ];
    let expected_hg_manifest_id_user = ManifestId::from("5d30a65c45e695416c96abfbd745f43c711879bb");
    let expected_hg_manifest_id_restricted =
        ManifestId::from("e53be16502cbc6afeb30ef30de7f6d9841fd4cb1");

    RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![
            ("restricted/one/a", None),
            ("user_project/foo/b", None),
        ])
        .expecting_manifest_id_store_entries(vec![
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_hg_manifest_id_user.clone(),
                RepoPath::dir("user_project/foo")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_hg_manifest_id_restricted.clone(),
                RepoPath::dir("restricted/one")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::HgAugmented,
                expected_hg_manifest_id_restricted.clone(),
                RepoPath::dir("restricted/one")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::HgAugmented,
                expected_hg_manifest_id_user.clone(),
                RepoPath::dir("user_project/foo")?,
            )?,
        ])
        .expecting_scuba_access_logs(vec![
            // user_project/foo access - HgManifest log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["user_project/foo"]),
                manifest_id: Some(expected_hg_manifest_id_user.clone()),
                manifest_type: Some(ManifestType::Hg),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User had access to this restricted path
                has_authorization: true,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![myusername_project_acl.clone()],
            },
            // user_project/foo access - HgAugmentedManifest log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["user_project/foo"]),
                manifest_id: Some(expected_hg_manifest_id_user.clone()),
                manifest_type: Some(ManifestType::HgAugmented),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User had access to this restricted path
                has_authorization: true,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![myusername_project_acl.clone()],
            },
            // user_project/foo access - path log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["user_project/foo"]),
                manifest_id: None,
                manifest_type: None,
                full_path: Some(NonRootMPath::new("user_project/foo")?),
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User had access to this restricted path
                has_authorization: true,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![myusername_project_acl.clone()],
            },
            // restricted/one access - HgManifest log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/one"]),
                manifest_id: Some(expected_hg_manifest_id_restricted.clone()),
                manifest_type: Some(ManifestType::Hg),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
            // restricted/one access - HgAugmentedManifest log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/one"]),
                manifest_id: Some(expected_hg_manifest_id_restricted.clone()),
                manifest_type: Some(ManifestType::HgAugmented),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
            // restricted/one access - path log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted/one"]),
                manifest_id: None,
                manifest_type: None,
                full_path: Some(NonRootMPath::new("restricted/one")?),
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
        ])
        .build(fb)
        .await?
        .run_restricted_paths_test()
        .await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_overlapping_restricted_directories(fb: FacebookInit) -> Result<()> {
    // Set up overlapping restricted paths: project/restricted is nested inside project
    let more_restricted_acl = MononokeIdentity::from_str("REPO_REGION:more_restricted_acl")?;
    let project_acl = MononokeIdentity::from_str("REPO_REGION:project_acl")?;
    let restricted_paths = vec![
        (
            NonRootMPath::new("project/restricted").unwrap(),
            more_restricted_acl.clone(),
        ),
        (NonRootMPath::new("project").unwrap(), project_acl.clone()),
    ];

    let expected_hg_manifest_id_root = ManifestId::from("0825286967058d61feb5b0031f4c23fa0a999965");
    let expected_hg_manifest_id_subdir =
        ManifestId::from("5629398cf56074c359a05b1f170eb2590efe11c3");

    // Access a file in the more restricted nested path - this should trigger both ACL checks
    // Custom ACL that gives access to project but NOT to project/restricted
    RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_test_acls(vec![
            ("project_acl", vec!["myusername0"]),
            ("more_restricted_acl", vec!["other_user"]),
        ])
        .with_file_path_changes(vec![("project/restricted/sensitive_file.txt", None)])
        .expecting_manifest_id_store_entries(vec![
            RestrictedPathManifestIdEntry::new(
                ManifestType::HgAugmented,
                expected_hg_manifest_id_root.clone(),
                RepoPath::dir("project")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::HgAugmented,
                expected_hg_manifest_id_subdir.clone(),
                RepoPath::dir("project/restricted")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_hg_manifest_id_root.clone(),
                RepoPath::dir("project")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_hg_manifest_id_subdir.clone(),
                RepoPath::dir("project/restricted")?,
            )?,
        ])
        .expecting_scuba_access_logs(vec![
            // project access - HgManifest log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["project"]),
                manifest_id: Some(expected_hg_manifest_id_root.clone()),
                manifest_type: Some(ManifestType::Hg),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User has access to the broader project ACL
                has_authorization: true,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![project_acl.clone()],
            },
            // project access - HgAugmentedManifest log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["project"]),
                manifest_id: Some(expected_hg_manifest_id_root.clone()),
                manifest_type: Some(ManifestType::HgAugmented),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User has access to the broader project ACL
                has_authorization: true,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![project_acl.clone()],
            },
            // project access - path log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["project"]),
                manifest_id: None,
                manifest_type: None,
                full_path: Some(NonRootMPath::new("project")?),
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User has access to the broader project ACL
                has_authorization: true,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![project_acl.clone()],
            },
            // project/restricted access - HgManifest log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["project/restricted"]),
                manifest_id: Some(expected_hg_manifest_id_subdir.clone()),
                manifest_type: Some(ManifestType::Hg),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User does NOT have access to the more restricted ACL
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![more_restricted_acl.clone()],
            },
            // project/restricted access - HgAugmentedManifest log
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["project/restricted"]),
                manifest_id: Some(expected_hg_manifest_id_subdir.clone()),
                manifest_type: Some(ManifestType::HgAugmented),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User does NOT have access to the more restricted ACL
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![more_restricted_acl.clone()],
            },
            // project/restricted access - path log (includes both ACLs)
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["project", "project/restricted"]),
                manifest_id: None,
                manifest_type: None,
                full_path: Some(NonRootMPath::new("project/restricted")?),
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User has access to project ACL, so overall authorization is true
                has_authorization: true,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![more_restricted_acl.clone(), project_acl.clone()],
            },
        ])
        .build(fb)
        .await?
        .run_restricted_paths_test()
        .await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_same_manifest_id_restricted_and_unrestricted_paths(fb: FacebookInit) -> Result<()> {
    // Set up a restricted path for the "restricted" directory
    let restricted_acl = MononokeIdentity::from_str("REPO_REGION:restricted_acl")?;
    let restricted_paths = vec![(
        NonRootMPath::new("restricted").unwrap(),
        restricted_acl.clone(),
    )];

    // Create two files with the same content in directories that should have the same manifest ID:
    // - restricted/foo/bar (under restricted path)
    // - unrestricted/foo/bar (not under restricted path)
    // Both foo/bar subdirectories should have identical manifest IDs since they contain identical content
    let identical_content = "same file content";

    let expected_manifest_id = ManifestId::from("0464bc4205fd3b4651678b66778299a352bac0d8");

    RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![
            ("restricted/foo/bar", Some(identical_content)),
            ("unrestricted/foo/bar", Some(identical_content)),
        ])
        .expecting_manifest_id_store_entries(vec![
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_manifest_id.clone(),
                RepoPath::dir("restricted")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::HgAugmented,
                expected_manifest_id.clone(),
                RepoPath::dir("restricted")?,
            )?,
        ])
        // In this scenario, there will be two logs of each manifest type for
        // restricted path. One for accessing the actual restricted path and
        // another for accessing the unrestricted path that has the same manifest ID.
        .expecting_scuba_access_logs(vec![
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted"]),
                manifest_id: Some(expected_manifest_id.clone()),
                manifest_type: Some(ManifestType::Hg),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User does NOT have access to restricted_acl
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted"]),
                manifest_id: Some(expected_manifest_id.clone()),
                manifest_type: Some(ManifestType::HgAugmented),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User does NOT have access to restricted_acl
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted"]),
                manifest_id: Some(expected_manifest_id.clone()),
                manifest_type: Some(ManifestType::Hg),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User does NOT have access to restricted_acl
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted"]),
                manifest_id: Some(expected_manifest_id.clone()),
                manifest_type: Some(ManifestType::HgAugmented),
                full_path: None,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User does NOT have access to restricted_acl
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
            // Path access logs - for directories traversed
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted"]),
                manifest_id: None,
                manifest_type: None,
                full_path: Some(NonRootMPath::new("restricted")?),
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User does NOT have access to restricted_acl
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: cast_to_non_root_mpaths(vec!["restricted"]),
                manifest_id: None,
                manifest_type: None,
                full_path: Some(NonRootMPath::new("restricted/foo")?),
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User does NOT have access to restricted_acl
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
                acls: vec![restricted_acl.clone()],
            },
        ])
        .build(fb)
        .await?
        .run_restricted_paths_test()
        .await?;

    Ok(())
}

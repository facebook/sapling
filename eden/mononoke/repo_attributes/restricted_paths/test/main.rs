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
use mononoke_types::RepositoryId;
use permission_checker::MononokeIdentity;
use restricted_paths::*;

mod utils;
use utils::*;

#[mononoke::fbinit_test]
async fn test_mercurial_manifest_no_restricted_change(fb: FacebookInit) -> Result<()> {
    let restricted_paths = vec![(
        NonRootMPath::new("restricted/dir").unwrap(),
        MononokeIdentity::from_str("REPO_REGION:restricted_acl")?,
    )];
    let test_data = RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![("unrestricted/dir/a", None)])
        .build(fb)
        .await?;

    let (manifest_id_store_entries, scuba_logs) = test_data.run_hg_manifest_test().await?;

    assert!(
        manifest_id_store_entries.is_empty(),
        "Manifest id store should be empty"
    );

    assert!(
        scuba_logs.is_empty(),
        "No restricted paths being accessed, so there shouldn't be any scuba logs"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_mercurial_manifest_change_to_restricted_with_access_is_logged(
    fb: FacebookInit,
) -> Result<()> {
    let restricted_paths = vec![(
        NonRootMPath::new("user_project/foo").unwrap(),
        MononokeIdentity::from_str("REPO_REGION:myusername_project")?,
    )];
    let test_data = RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![("user_project/foo/bar/a", None)])
        .build(fb)
        .await?;

    let (manifest_id_store_entries, scuba_logs) = test_data.run_hg_manifest_test().await?;

    let expected_manifest_id = ManifestId::from("f15543536ef8c0578589b6aa5a85e49233f38a6b");

    pretty_assertions::assert_eq!(
        manifest_id_store_entries,
        vec![RestrictedPathManifestIdEntry::new(
            ManifestType::Hg,
            expected_manifest_id.clone(),
            NonRootMPath::new("user_project/foo")?
        )]
    );

    pretty_assertions::assert_eq!(
        scuba_logs,
        vec![ScubaAccessLogSample {
            repo_id: RepositoryId::new(0),
            // The restricted path root is logged, not the full path
            restricted_paths: vec!["user_project/foo"]
                .into_iter()
                .map(NonRootMPath::new)
                .collect::<Result<Vec<_>>>()?,
            manifest_id: expected_manifest_id,
            manifest_type: ManifestType::Hg,
            client_identities: vec!["USER:myusername0"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
            has_authorization: true,
            client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
        },]
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_mercurial_manifest_single_dir_single_restricted_change(
    fb: FacebookInit,
) -> Result<()> {
    let restricted_paths = vec![(
        NonRootMPath::new("restricted/dir").unwrap(),
        MononokeIdentity::from_str("REPO_REGION:restricted_acl")?,
    )];
    let test_data = RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![("restricted/dir/a", None)])
        .build(fb)
        .await?;

    let (manifest_id_store_entries, scuba_logs) = test_data.run_hg_manifest_test().await?;

    let expected_manifest_id = ManifestId::from("0e3837eaab4fb0454c78f290aeb747a201ccd05b");

    pretty_assertions::assert_eq!(
        manifest_id_store_entries,
        vec![RestrictedPathManifestIdEntry::new(
            ManifestType::Hg,
            expected_manifest_id.clone(),
            NonRootMPath::new("restricted/dir")?
        )]
    );

    pretty_assertions::assert_eq!(
        scuba_logs,
        vec![ScubaAccessLogSample {
            repo_id: RepositoryId::new(0),
            restricted_paths: vec!["restricted/dir"]
                .into_iter()
                .map(NonRootMPath::new)
                .collect::<Result<Vec<_>>>()?,
            manifest_id: expected_manifest_id,
            manifest_type: ManifestType::Hg,
            client_identities: vec!["USER:myusername0"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
            has_authorization: false,
            client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
        },]
    );

    Ok(())
}

// Multiple files in a single restricted directory generate a single entry in
// the manifest id store.
#[mononoke::fbinit_test]
async fn test_mercurial_manifest_single_dir_many_restricted_changes(
    fb: FacebookInit,
) -> Result<()> {
    let restricted_paths = vec![(
        NonRootMPath::new("restricted/dir").unwrap(),
        MononokeIdentity::from_str("REPO_REGION:restricted_acl")?,
    )];
    let test_data = RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![("restricted/dir/a", None), ("restricted/dir/b", None)])
        .build(fb)
        .await?;

    let (manifest_id_store_entries, scuba_logs) = test_data.run_hg_manifest_test().await?;

    let expected_manifest_id = ManifestId::from("3132e75d8439632fc89f193cbf4f02b2b5428c6e");

    pretty_assertions::assert_eq!(
        manifest_id_store_entries,
        vec![RestrictedPathManifestIdEntry::new(
            ManifestType::Hg,
            expected_manifest_id.clone(),
            NonRootMPath::new("restricted/dir")?
        )]
    );

    pretty_assertions::assert_eq!(
        scuba_logs,
        vec![
            // Single log entry for both files, because they're under the same
            // restricted directory
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: vec!["restricted/dir"]
                    .into_iter()
                    .map(NonRootMPath::new)
                    .collect::<Result<Vec<_>>>()?,
                manifest_id: expected_manifest_id.clone(),
                manifest_type: ManifestType::Hg,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
            },
        ]
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_mercurial_manifest_single_dir_restricted_and_unrestricted(
    fb: FacebookInit,
) -> Result<()> {
    let restricted_paths = vec![(
        NonRootMPath::new("restricted/dir").unwrap(),
        MononokeIdentity::from_str("REPO_REGION:restricted_acl")?,
    )];
    let test_data = RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![
            ("restricted/dir/a", None),
            ("unrestricted/dir/b", None),
        ])
        .build(fb)
        .await?;

    let (manifest_id_store_entries, scuba_logs) = test_data.run_hg_manifest_test().await?;

    let expected_manifest_id = ManifestId::from("0e3837eaab4fb0454c78f290aeb747a201ccd05b");

    pretty_assertions::assert_eq!(
        manifest_id_store_entries,
        vec![RestrictedPathManifestIdEntry::new(
            ManifestType::Hg,
            expected_manifest_id.clone(),
            NonRootMPath::new("restricted/dir")?
        ),]
    );

    pretty_assertions::assert_eq!(
        scuba_logs,
        vec![ScubaAccessLogSample {
            repo_id: RepositoryId::new(0),
            restricted_paths: vec!["restricted/dir"]
                .into_iter()
                .map(NonRootMPath::new)
                .collect::<Result<Vec<_>>>()?,
            manifest_id: expected_manifest_id,
            manifest_type: ManifestType::Hg,
            client_identities: vec!["USER:myusername0"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
            has_authorization: false,
            client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
        },]
    );

    Ok(())
}

// Multiple restricted directories generate multiple entries in the manifest
#[mononoke::fbinit_test]
async fn test_mercurial_manifest_multiple_restricted_dirs(fb: FacebookInit) -> Result<()> {
    let restricted_paths = vec![
        (
            NonRootMPath::new("restricted/one").unwrap(),
            MononokeIdentity::from_str("REPO_REGION:restricted_acl")?,
        ),
        (
            NonRootMPath::new("restricted/two").unwrap(),
            MononokeIdentity::from_str("REPO_REGION:another_acl")?,
        ),
    ];
    let test_data = RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![("restricted/one/a", None), ("restricted/two/b", None)])
        .build(fb)
        .await?;

    let (manifest_id_store_entries, scuba_logs) = test_data.run_hg_manifest_test().await?;

    let expected_manifest_id_one = ManifestId::from("e53be16502cbc6afeb30ef30de7f6d9841fd4cb1");
    let expected_manifest_id_two = ManifestId::from("f5ca206223b4d531f0d65ff422273f901bc7a024");

    pretty_assertions::assert_eq!(
        manifest_id_store_entries,
        vec![
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_manifest_id_one.clone(),
                NonRootMPath::new("restricted/one")?
            ),
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_manifest_id_two.clone(),
                NonRootMPath::new("restricted/two")?
            ),
        ]
    );

    pretty_assertions::assert_eq!(
        scuba_logs,
        vec![
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: vec!["restricted/two"]
                    .into_iter()
                    .map(NonRootMPath::new)
                    .collect::<Result<Vec<_>>>()?,
                manifest_id: expected_manifest_id_two,
                manifest_type: ManifestType::Hg,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
            },
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: vec!["restricted/one"]
                    .into_iter()
                    .map(NonRootMPath::new)
                    .collect::<Result<Vec<_>>>()?,
                manifest_id: expected_manifest_id_one,
                manifest_type: ManifestType::Hg,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
            },
        ]
    );

    Ok(())
}

// Test that if the user has access to one of the restricted paths, there will
// be a log entry for each one with the proper authorization result.
#[mononoke::fbinit_test]
async fn test_mercurial_manifest_multiple_restricted_dirs_with_partial_access(
    fb: FacebookInit,
) -> Result<()> {
    let restricted_paths = vec![
        (
            NonRootMPath::new("restricted/one").unwrap(),
            MononokeIdentity::from_str("REPO_REGION:restricted_acl")?,
        ),
        (
            // User will have access to this path
            NonRootMPath::new("user_project/foo").unwrap(),
            MononokeIdentity::from_str("REPO_REGION:myusername_project")?,
        ),
    ];
    let test_data = RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![
            ("restricted/one/a", None),
            ("user_project/foo/b", None),
        ])
        .build(fb)
        .await?;

    let (manifest_id_store_entries, scuba_logs) = test_data.run_hg_manifest_test().await?;

    let expected_authorized_manifest_id =
        ManifestId::from("5d30a65c45e695416c96abfbd745f43c711879bb");
    let expected_unauthorized_manifest_id =
        ManifestId::from("e53be16502cbc6afeb30ef30de7f6d9841fd4cb1");

    pretty_assertions::assert_eq!(
        manifest_id_store_entries,
        vec![
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_authorized_manifest_id.clone(),
                NonRootMPath::new("user_project/foo")?
            ),
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_unauthorized_manifest_id.clone(),
                NonRootMPath::new("restricted/one")?
            ),
        ]
    );

    pretty_assertions::assert_eq!(
        scuba_logs,
        vec![
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: vec!["user_project/foo"]
                    .into_iter()
                    .map(NonRootMPath::new)
                    .collect::<Result<Vec<_>>>()?,
                manifest_id: expected_authorized_manifest_id,
                manifest_type: ManifestType::Hg,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User had access to this restricted path
                has_authorization: true,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
            },
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: vec!["restricted/one"]
                    .into_iter()
                    .map(NonRootMPath::new)
                    .collect::<Result<Vec<_>>>()?,
                manifest_id: expected_unauthorized_manifest_id,
                manifest_type: ManifestType::Hg,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
            },
        ]
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_mercurial_manifest_overlapping_restricted_directories(
    fb: FacebookInit,
) -> Result<()> {
    // Set up overlapping restricted paths: project/restricted is nested inside project
    let restricted_paths = vec![
        (
            NonRootMPath::new("project/restricted").unwrap(),
            MononokeIdentity::from_str("REPO_REGION:more_restricted_acl")?,
        ),
        (
            NonRootMPath::new("project").unwrap(),
            MononokeIdentity::from_str("REPO_REGION:project_acl")?,
        ),
    ];

    // Custom ACL that gives access to project but NOT to project/restricted
    let custom_acl = r#"{
  "repos": {
    "default": {
      "actions": {
        "read": ["USER:myusername0"],
        "write": ["USER:myusername0"]
      }
    }
  },
  "repo_regions": {
    "project_acl": {
      "actions": {
        "read": ["USER:myusername0"]
      }
    },
    "more_restricted_acl": {
      "actions": {
        "read": ["USER:other_user"]
      }
    }
  }
}"#;

    let test_data = RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_acl_json(Some(custom_acl))
        .with_file_path_changes(vec![("project/restricted/sensitive_file.txt", None)])
        .build(fb)
        .await?;

    // Access a file in the more restricted nested path - this should trigger both ACL checks
    let (manifest_id_store_entries, scuba_logs) = test_data.run_hg_manifest_test().await?;

    let expected_manifest_id_root = ManifestId::from("0825286967058d61feb5b0031f4c23fa0a999965");
    let expected_manifest_id_subdir = ManifestId::from("5629398cf56074c359a05b1f170eb2590efe11c3");

    // Should have manifest entries for both overlapping paths
    pretty_assertions::assert_eq!(
        manifest_id_store_entries,
        vec![
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_manifest_id_root.clone(),
                NonRootMPath::new("project")?
            ),
            RestrictedPathManifestIdEntry::new(
                ManifestType::Hg,
                expected_manifest_id_subdir.clone(),
                NonRootMPath::new("project/restricted")?
            ),
        ]
    );

    // Should log access to both overlapping paths:
    // - project/restricted (unauthorized - user doesn't have more_restricted_acl)
    // - project (authorized - user has project_acl)
    pretty_assertions::assert_eq!(
        scuba_logs,
        vec![
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: vec!["project"]
                    .into_iter()
                    .map(NonRootMPath::new)
                    .collect::<Result<Vec<_>>>()?,
                manifest_id: expected_manifest_id_root,
                manifest_type: ManifestType::Hg,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User has access to the broader project ACL
                has_authorization: true,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
            },
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: vec!["project/restricted"]
                    .into_iter()
                    .map(NonRootMPath::new)
                    .collect::<Result<Vec<_>>>()?,
                manifest_id: expected_manifest_id_subdir,
                manifest_type: ManifestType::Hg,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User does NOT have access to the more restricted ACL
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
            },
        ]
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_mercurial_manifest_same_manifest_id_restricted_and_unrestricted_paths(
    fb: FacebookInit,
) -> Result<()> {
    // Set up a restricted path for the "restricted" directory
    let restricted_paths = vec![(
        NonRootMPath::new("restricted").unwrap(),
        MononokeIdentity::from_str("REPO_REGION:restricted_acl")?,
    )];

    // Create two files with the same content in directories that should have the same manifest ID:
    // - restricted/foo/bar (under restricted path)
    // - unrestricted/foo/bar (not under restricted path)
    // Both foo/bar subdirectories should have identical manifest IDs since they contain identical content
    let identical_content = "same file content";
    let test_data = RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![
            ("restricted/foo/bar", Some(identical_content)),
            ("unrestricted/foo/bar", Some(identical_content)),
        ])
        .build(fb)
        .await?;

    let (manifest_id_store_entries, scuba_logs) = test_data.run_hg_manifest_test().await?;

    let expected_manifest_id = ManifestId::from("0464bc4205fd3b4651678b66778299a352bac0d8");

    // Should have manifest entry for the restricted path only
    pretty_assertions::assert_eq!(
        manifest_id_store_entries,
        vec![RestrictedPathManifestIdEntry::new(
            ManifestType::Hg,
            expected_manifest_id.clone(),
            NonRootMPath::new("restricted")?
        ),]
    );

    // The helper function will access all directories by manifest ID. It will
    // encounter the manifest ID for "foo/bar" twice, once for the restricted
    // and once for the restricted directory. But it will log both samples as
    // if the user was accessing the restricted directory, i.e. as unauthorized
    // access.
    pretty_assertions::assert_eq!(
        scuba_logs,
        vec![
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: vec!["restricted"]
                    .into_iter()
                    .map(NonRootMPath::new)
                    .collect::<Result<Vec<_>>>()?,
                manifest_id: expected_manifest_id.clone(),
                manifest_type: ManifestType::Hg,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User does NOT have access to restricted_acl
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
            },
            ScubaAccessLogSample {
                repo_id: RepositoryId::new(0),
                restricted_paths: vec!["restricted"]
                    .into_iter()
                    .map(NonRootMPath::new)
                    .collect::<Result<Vec<_>>>()?,
                manifest_id: expected_manifest_id.clone(),
                manifest_type: ManifestType::Hg,
                client_identities: vec!["USER:myusername0"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                // User does NOT have access to restricted_acl
                has_authorization: false,
                client_main_id: TEST_CLIENT_MAIN_ID.to_string(),
            },
        ]
    );

    Ok(())
}

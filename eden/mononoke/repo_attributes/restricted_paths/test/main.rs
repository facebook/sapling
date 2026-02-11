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

    // Base sample with fields common to ALL expected samples
    let base_sample = ScubaAccessLogSampleBuilder::new()
        .with_repo_id(RepositoryId::new(0))
        .with_client_identities(
            vec!["USER:myusername0"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
        )
        .with_client_main_id(TEST_CLIENT_MAIN_ID.to_string());

    let expected_fsnode_id =
        ManifestId::from("e11f63c6ae1c9d8c6e8460805c4f549b9c324c9f17abe12398338a2be32a7977");

    RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![("user_project/foo/bar/a", None)])
        .with_test_groups(vec![
            // Group ACLs to conditionally enable enforcement of restricted paths
            // i.e. throw AuthorizationError when trying to fetch unauthorized paths
            ("enforcement_acl", vec!["USER:myusername0"]),
        ])
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
            RestrictedPathManifestIdEntry::new(
                ManifestType::Fsnode,
                expected_fsnode_id.clone(),
                RepoPath::dir("user_project/foo")?,
            )?,
        ])
        .expecting_scuba_access_logs(vec![
            // HgManifest access log
            base_sample
                .clone()
                // The restricted path root is logged, not the full path
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["user_project/foo"]))
                .with_manifest_id(expected_manifest_id.clone())
                .with_manifest_type(ManifestType::Hg)
                .with_has_authorization(true)
                .with_acls(vec![project_acl.clone()])
                .build()?,
            // HgAugmentedManifest access log
            base_sample
                .clone()
                // The restricted path root is logged, not the full path
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["user_project/foo"]))
                .with_manifest_id(expected_manifest_id.clone())
                .with_manifest_type(ManifestType::HgAugmented)
                .with_has_authorization(true)
                .with_acls(vec![project_acl.clone()])
                .build()?,
            // Path access logs for directories traversed
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["user_project/foo"]))
                .with_full_path(NonRootMPath::new("user_project/foo")?)
                .with_has_authorization(true)
                .with_acls(vec![project_acl.clone()])
                .build()?,
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["user_project/foo"]))
                .with_full_path(NonRootMPath::new("user_project/foo/bar")?)
                .with_has_authorization(true)
                .with_acls(vec![project_acl.clone()])
                .build()?,
            // Fsnode access log
            base_sample
                .clone()
                // The restricted path root is logged, not the full path
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["user_project/foo"]))
                .with_manifest_id(expected_fsnode_id.clone())
                .with_manifest_type(ManifestType::Fsnode)
                .with_has_authorization(true)
                .with_acls(vec![project_acl.clone()])
                .build()?,
            // Path access logs for directories traversed
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["user_project/foo"]))
                .with_full_path(NonRootMPath::new("user_project/foo/bar/a")?)
                .with_has_authorization(true)
                .with_acls(vec![project_acl.clone()])
                .build()?,
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["user_project/foo"]))
                .with_full_path(NonRootMPath::new("user_project/foo/bar/a")?)
                .with_has_authorization(true)
                .with_acls(vec![project_acl.clone()])
                .build()?,
        ])
        .with_enforcement_scenarios(vec![
            // Matching ACL = enforcement enabled.
            (
                vec![MononokeIdentity::new("GROUP", "enforcement_acl")],
                // Enforcement is enabled, but user has access to the restricted
                // path, so no AuthorizationError is thrown.
                false,
            ),
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

    // Base sample with fields common to ALL expected samples
    let base_sample = ScubaAccessLogSampleBuilder::new()
        .with_repo_id(RepositoryId::new(0))
        .with_client_identities(
            vec!["USER:myusername0"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
        )
        .with_client_main_id(TEST_CLIENT_MAIN_ID.to_string());

    let expected_fsnode_id =
        ManifestId::from("537548f0637858f6ebbba3e7f6c4d0c4e1ee7f88ca50fe3acff964115de0a0a3");

    RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_file_path_changes(vec![("restricted/dir/a", None)])
        .with_test_groups(vec![
            // Group ACLs to conditionally enable enforcement of restricted paths
            // i.e. throw AuthorizationError when trying to fetch unauthorized paths
            ("enforcement_acl", vec!["USER:myusername0"]),
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
            RestrictedPathManifestIdEntry::new(
                ManifestType::Fsnode,
                expected_fsnode_id.clone(),
                RepoPath::dir("restricted/dir")?,
            )?,
        ])
        .expecting_scuba_access_logs(vec![
            // HgManifest access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_manifest_id(expected_manifest_id.clone())
                .with_manifest_type(ManifestType::Hg)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // HgAugmentedManifest access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_manifest_id(expected_manifest_id.clone())
                .with_manifest_type(ManifestType::HgAugmented)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Path access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Fsnode access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_manifest_id(expected_fsnode_id.clone())
                .with_manifest_type(ManifestType::Fsnode)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Path fsnode access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir/a")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir/a")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
        ])
        // The test user has identity USER:myusername0 and client_main_id "user:myusername0"
        // Enforcement is based on ACL membership via conditional_enforcement_acls
        .with_enforcement_scenarios(vec![
            // No ACLs = no enforcement (logging only)
            (vec![], false),
            // Non-matching ACL = no enforcement (user not in this ACL)
            (
                vec![MononokeIdentity::new("REPO_REGION", "nonexistent_acl")],
                false,
            ),
            // Matching ACL = enforcement triggered
            // The test user is a member of the "enforcement_acl" repo region
            (
                vec![MononokeIdentity::new("GROUP", "enforcement_acl")],
                true,
            ),
            // Multiple ACLs are OR'd together: if any ACL matches, enforcement is triggered
            (
                vec![
                    MononokeIdentity::new("GROUP", "nonexistent_acl"),
                    MononokeIdentity::new("GROUP", "enforcement_acl"),
                ],
                true,
            ),
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

    let expected_fsnode_id =
        ManifestId::from("34cc689ef0f1eeb886531a16d5953939e19d46fe6b16f7d7056d4c02a1c572ae");

    // Base sample with fields common to ALL expected samples
    let base_sample = ScubaAccessLogSampleBuilder::new()
        .with_repo_id(RepositoryId::new(0))
        .with_client_identities(
            vec!["USER:myusername0"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
        )
        .with_client_main_id(TEST_CLIENT_MAIN_ID.to_string());

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
            RestrictedPathManifestIdEntry::new(
                ManifestType::Fsnode,
                expected_fsnode_id.clone(),
                RepoPath::dir("restricted/dir")?,
            )?,
        ])
        .expecting_scuba_access_logs(vec![
            // HgManifest access log - Single log entry for both files, because they're under the same
            // restricted directory
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_manifest_id(expected_manifest_id.clone())
                .with_manifest_type(ManifestType::Hg)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // HgAugmentedManifest access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_manifest_id(expected_manifest_id.clone())
                .with_manifest_type(ManifestType::HgAugmented)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Path access log - for the directory containing both files
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Fsnode access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_manifest_id(expected_fsnode_id.clone())
                .with_manifest_type(ManifestType::Fsnode)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Path access log - for the directory containing both files
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir/a")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Path access log - for the directory containing both files
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir/b")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Blame access log - for the directory containing both files
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir/a")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Blame access log - for the directory containing both files
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir/b")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
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

    // Base sample with fields common to ALL expected samples
    let base_sample = ScubaAccessLogSampleBuilder::new()
        .with_repo_id(RepositoryId::new(0))
        .with_client_identities(
            vec!["USER:myusername0"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
        )
        .with_client_main_id(TEST_CLIENT_MAIN_ID.to_string());

    let expected_fsnode_id =
        ManifestId::from("537548f0637858f6ebbba3e7f6c4d0c4e1ee7f88ca50fe3acff964115de0a0a3");

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
            RestrictedPathManifestIdEntry::new(
                ManifestType::Fsnode,
                expected_fsnode_id.clone(),
                RepoPath::dir("restricted/dir")?,
            )?,
        ])
        .expecting_scuba_access_logs(vec![
            // HgManifest access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_manifest_id(expected_manifest_id.clone())
                .with_manifest_type(ManifestType::Hg)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // HgAugmentedManifest access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_manifest_id(expected_manifest_id.clone())
                .with_manifest_type(ManifestType::HgAugmented)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Path access log - only for restricted directory
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Fsnode access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_manifest_id(expected_fsnode_id.clone())
                .with_manifest_type(ManifestType::Fsnode)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Path fsnode access log - only for restricted directory
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir/a")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Blame access logs
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir/a")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
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

    let expected_fsnode_id_one =
        ManifestId::from("5acb66d820607d6caa806153c7471b3499ca21d8c5eeff5078fa8f0403fe4f13");
    let expected_fsnode_id_two =
        ManifestId::from("e4351278ef7c2029a40ca6cbd6132e675d3e5199cfd0b16a7eba80d648221054");

    // Base sample with fields common to ALL expected samples
    let base_sample = ScubaAccessLogSampleBuilder::new()
        .with_repo_id(RepositoryId::new(0))
        .with_client_identities(
            vec!["USER:myusername0"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
        )
        .with_client_main_id(TEST_CLIENT_MAIN_ID.to_string());

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
            RestrictedPathManifestIdEntry::new(
                ManifestType::Fsnode,
                expected_fsnode_id_one.clone(),
                RepoPath::dir("restricted/one")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::Fsnode,
                expected_fsnode_id_two.clone(),
                RepoPath::dir("restricted/two")?,
            )?,
        ])
        .expecting_scuba_access_logs(vec![
            // restricted/two access - HgManifest log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/two"]))
                .with_manifest_id(expected_hg_manifest_id_two.clone())
                .with_manifest_type(ManifestType::Hg)
                .with_has_authorization(false)
                .with_acls(vec![another_acl.clone()])
                .build()?,
            // restricted/two access - HgAugmentedManifest log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/two"]))
                .with_manifest_id(expected_hg_manifest_id_two.clone())
                .with_manifest_type(ManifestType::HgAugmented)
                .with_has_authorization(false)
                .with_acls(vec![another_acl.clone()])
                .build()?,
            // restricted/two access - path log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/two"]))
                .with_full_path(NonRootMPath::new("restricted/two")?)
                .with_has_authorization(false)
                .with_acls(vec![another_acl.clone()])
                .build()?,
            // restricted/one access - HgManifest log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/one"]))
                .with_manifest_id(expected_hg_manifest_id_one.clone())
                .with_manifest_type(ManifestType::Hg)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // restricted/one access - HgAugmentedManifest log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/one"]))
                .with_manifest_id(expected_hg_manifest_id_one.clone())
                .with_manifest_type(ManifestType::HgAugmented)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // restricted/one access - path log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/one"]))
                .with_full_path(NonRootMPath::new("restricted/one")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // restricted/two access - Fsnode log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/two"]))
                .with_manifest_id(expected_fsnode_id_two.clone())
                .with_manifest_type(ManifestType::Fsnode)
                .with_has_authorization(false)
                .with_acls(vec![another_acl.clone()])
                .build()?,
            // restricted/one access - Fsnode log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/one"]))
                .with_manifest_id(expected_fsnode_id_one.clone())
                .with_manifest_type(ManifestType::Fsnode)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // restricted/two access - path log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/two"]))
                .with_full_path(NonRootMPath::new("restricted/two/b")?)
                .with_has_authorization(false)
                .with_acls(vec![another_acl.clone()])
                .build()?,
            // restricted/one access - path log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/one"]))
                .with_full_path(NonRootMPath::new("restricted/one/a")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // restricted/two access - path log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/two"]))
                .with_full_path(NonRootMPath::new("restricted/two/b")?)
                .with_has_authorization(false)
                .with_acls(vec![another_acl.clone()])
                .build()?,
            // restricted/one access - path log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/one"]))
                .with_full_path(NonRootMPath::new("restricted/one/a")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
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

    let expected_fsnode_id_user =
        ManifestId::from("f8f67a4a72a3bbd512be9a9348614c555cc6ac6a21570ce6c7f93c89a623157b");
    let expected_fsnode_id_restricted =
        ManifestId::from("5acb66d820607d6caa806153c7471b3499ca21d8c5eeff5078fa8f0403fe4f13");

    // Base sample with fields common to ALL expected samples
    let base_sample = ScubaAccessLogSampleBuilder::new()
        .with_repo_id(RepositoryId::new(0))
        .with_client_identities(
            vec!["USER:myusername0"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
        )
        .with_client_main_id(TEST_CLIENT_MAIN_ID.to_string());

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
            RestrictedPathManifestIdEntry::new(
                ManifestType::Fsnode,
                expected_fsnode_id_restricted.clone(),
                RepoPath::dir("restricted/one")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::Fsnode,
                expected_fsnode_id_user.clone(),
                RepoPath::dir("user_project/foo")?,
            )?,
        ])
        .expecting_scuba_access_logs(vec![
            // user_project/foo access - HgManifest log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["user_project/foo"]))
                .with_manifest_id(expected_hg_manifest_id_user.clone())
                .with_manifest_type(ManifestType::Hg)
                // User had access to this restricted path
                .with_has_authorization(true)
                .with_acls(vec![myusername_project_acl.clone()])
                .build()?,
            // user_project/foo access - HgAugmentedManifest log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["user_project/foo"]))
                .with_manifest_id(expected_hg_manifest_id_user.clone())
                .with_manifest_type(ManifestType::HgAugmented)
                // User had access to this restricted path
                .with_has_authorization(true)
                .with_acls(vec![myusername_project_acl.clone()])
                .build()?,
            // user_project/foo access - path log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["user_project/foo"]))
                .with_full_path(NonRootMPath::new("user_project/foo")?)
                // User had access to this restricted path
                .with_has_authorization(true)
                .with_acls(vec![myusername_project_acl.clone()])
                .build()?,
            // restricted/one access - HgManifest log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/one"]))
                .with_manifest_id(expected_hg_manifest_id_restricted.clone())
                .with_manifest_type(ManifestType::Hg)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // restricted/one access - HgAugmentedManifest log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/one"]))
                .with_manifest_id(expected_hg_manifest_id_restricted.clone())
                .with_manifest_type(ManifestType::HgAugmented)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // restricted/one access - path log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/one"]))
                .with_full_path(NonRootMPath::new("restricted/one")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // user_project/foo access - Fsnode log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["user_project/foo"]))
                .with_manifest_id(expected_fsnode_id_user.clone())
                .with_manifest_type(ManifestType::Fsnode)
                // User had access to this restricted path
                .with_has_authorization(true)
                .with_acls(vec![myusername_project_acl.clone()])
                .build()?,
            // restricted/one access - Fsnode log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/one"]))
                .with_manifest_id(expected_fsnode_id_restricted.clone())
                .with_manifest_type(ManifestType::Fsnode)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // user_project/foo access - path log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["user_project/foo"]))
                .with_full_path(NonRootMPath::new("user_project/foo/b")?)
                // User had access to this restricted path
                .with_has_authorization(true)
                .with_acls(vec![myusername_project_acl.clone()])
                .build()?,
            // restricted/one access - path log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/one"]))
                .with_full_path(NonRootMPath::new("restricted/one/a")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // user_project/foo access - path log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["user_project/foo"]))
                .with_full_path(NonRootMPath::new("user_project/foo/b")?)
                // User had access to this restricted path
                .with_has_authorization(true)
                .with_acls(vec![myusername_project_acl.clone()])
                .build()?,
            // restricted/one access - path log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/one"]))
                .with_full_path(NonRootMPath::new("restricted/one/a")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
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

    let expected_fsnode_id_root =
        ManifestId::from("2000ed8d83bc282584fce88e6a67e2c23f53b72d6eb41e32a89211e60c80e3b9");
    let expected_fsnode_id_subdir =
        ManifestId::from("a9d72b0ed490fe8da33d1ec0e7b6890a6447cc6ba220532927bb866eb9c36768");

    // Base sample with fields common to ALL expected samples
    let base_sample = ScubaAccessLogSampleBuilder::new()
        .with_repo_id(RepositoryId::new(0))
        .with_client_identities(
            vec!["USER:myusername0"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
        )
        .with_client_main_id(TEST_CLIENT_MAIN_ID.to_string());

    // Access a file in the more restricted nested path - this should trigger both ACL checks
    // Custom ACL that gives access to project but NOT to project/restricted
    RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_test_repo_region_acls(vec![
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
            RestrictedPathManifestIdEntry::new(
                ManifestType::Fsnode,
                expected_fsnode_id_root.clone(),
                RepoPath::dir("project")?,
            )?,
            RestrictedPathManifestIdEntry::new(
                ManifestType::Fsnode,
                expected_fsnode_id_subdir.clone(),
                RepoPath::dir("project/restricted")?,
            )?,
        ])
        .expecting_scuba_access_logs(vec![
            // project access - HgManifest log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["project"]))
                .with_manifest_id(expected_hg_manifest_id_root.clone())
                .with_manifest_type(ManifestType::Hg)
                // User has access to the broader project ACL
                .with_has_authorization(true)
                .with_acls(vec![project_acl.clone()])
                .build()?,
            // project access - HgAugmentedManifest log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["project"]))
                .with_manifest_id(expected_hg_manifest_id_root.clone())
                .with_manifest_type(ManifestType::HgAugmented)
                // User has access to the broader project ACL
                .with_has_authorization(true)
                .with_acls(vec![project_acl.clone()])
                .build()?,
            // project access - path log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["project"]))
                .with_full_path(NonRootMPath::new("project")?)
                // User has access to the broader project ACL
                .with_has_authorization(true)
                .with_acls(vec![project_acl.clone()])
                .build()?,
            // project/restricted access - HgManifest log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["project/restricted"]))
                .with_manifest_id(expected_hg_manifest_id_subdir.clone())
                .with_manifest_type(ManifestType::Hg)
                // User does NOT have access to the more restricted ACL
                .with_has_authorization(false)
                .with_acls(vec![more_restricted_acl.clone()])
                .build()?,
            // project/restricted access - HgAugmentedManifest log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["project/restricted"]))
                .with_manifest_id(expected_hg_manifest_id_subdir.clone())
                .with_manifest_type(ManifestType::HgAugmented)
                // User does NOT have access to the more restricted ACL
                .with_has_authorization(false)
                .with_acls(vec![more_restricted_acl.clone()])
                .build()?,
            // project/restricted access - path log (includes both ACLs)
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec![
                    "project",
                    "project/restricted",
                ]))
                .with_full_path(NonRootMPath::new("project/restricted")?)
                .with_has_authorization(true)
                .with_acls(vec![more_restricted_acl.clone(), project_acl.clone()])
                .build()?,
            // project access - Fsnode log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["project"]))
                .with_manifest_id(expected_fsnode_id_root.clone())
                .with_manifest_type(ManifestType::Fsnode)
                // User has access to the broader project ACL
                .with_has_authorization(true)
                .with_acls(vec![project_acl.clone()])
                .build()?,
            // project/restricted access - Fsnode log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["project/restricted"]))
                .with_manifest_id(expected_fsnode_id_subdir.clone())
                .with_manifest_type(ManifestType::Fsnode)
                // User has access to the broader project ACL
                .with_has_authorization(false)
                .with_acls(vec![more_restricted_acl.clone()])
                .build()?,
            // project/restricted access - path log (includes both ACLs)
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec![
                    "project",
                    "project/restricted",
                ]))
                .with_full_path(NonRootMPath::new("project/restricted/sensitive_file.txt")?)
                // User has access to the broader project ACL
                .with_has_authorization(true)
                .with_acls(vec![more_restricted_acl.clone(), project_acl.clone()])
                .build()?,
            // project/restricted access - path log (includes both ACLs)
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec![
                    "project",
                    "project/restricted",
                ]))
                .with_full_path(NonRootMPath::new("project/restricted/sensitive_file.txt")?)
                // User has access to the broader project ACL
                .with_has_authorization(true)
                .with_acls(vec![more_restricted_acl.clone(), project_acl.clone()])
                .build()?,
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
    let expected_fsnode_id =
        ManifestId::from("c259976b56b9826eea5ca4c3a68d4e43a2f1745884918a96ac0dabb635f73598");

    // Base sample with fields common to ALL expected samples
    let base_sample = ScubaAccessLogSampleBuilder::new()
        .with_repo_id(RepositoryId::new(0))
        .with_client_identities(
            vec!["USER:myusername0"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
        )
        .with_client_main_id(TEST_CLIENT_MAIN_ID.to_string());

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
            RestrictedPathManifestIdEntry::new(
                ManifestType::Fsnode,
                expected_fsnode_id.clone(),
                RepoPath::dir("restricted")?,
            )?,
        ])
        // In this scenario, there will be two logs of each manifest type for
        // restricted path. One for accessing the actual restricted path and
        // another for accessing the unrestricted path that has the same manifest ID.
        .expecting_scuba_access_logs(vec![
            // Two HgManifest access logs - for both files that trigger the same manifest access
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted"]))
                .with_manifest_id(expected_manifest_id.clone())
                .with_manifest_type(ManifestType::Hg)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted"]))
                .with_manifest_id(expected_manifest_id.clone())
                .with_manifest_type(ManifestType::HgAugmented)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted"]))
                .with_manifest_id(expected_manifest_id.clone())
                .with_manifest_type(ManifestType::Hg)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted"]))
                .with_manifest_id(expected_manifest_id.clone())
                .with_manifest_type(ManifestType::HgAugmented)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Path access logs - for directories traversed
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted"]))
                .with_full_path(NonRootMPath::new("restricted")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted"]))
                .with_full_path(NonRootMPath::new("restricted/foo")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted"]))
                .with_manifest_id(expected_fsnode_id.clone())
                .with_manifest_type(ManifestType::Fsnode)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted"]))
                .with_manifest_id(expected_fsnode_id.clone())
                .with_manifest_type(ManifestType::Fsnode)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted"]))
                .with_full_path(NonRootMPath::new("restricted/foo/bar")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted"]))
                .with_full_path(NonRootMPath::new("restricted/foo/bar")?)
                .with_has_authorization(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
        ])
        .build(fb)
        .await?
        .run_restricted_paths_test()
        .await?;

    Ok(())
}

// Test that is_allowlisted_tooling is set to true when the client is in the
// tooling allowlist group.
#[mononoke::fbinit_test]
async fn test_tooling_allowlist_acl_user_in_acl(fb: FacebookInit) -> Result<()> {
    // Service myservice0 has access to the tooling_group
    let restricted_acl = MononokeIdentity::from_str("REPO_REGION:restricted_acl")?;
    let restricted_paths = vec![(
        NonRootMPath::new("restricted/dir").unwrap(),
        restricted_acl.clone(),
    )];

    let expected_manifest_id = ManifestId::from("0e3837eaab4fb0454c78f290aeb747a201ccd05b");
    let expected_fsnode_id =
        ManifestId::from("537548f0637858f6ebbba3e7f6c4d0c4e1ee7f88ca50fe3acff964115de0a0a3");

    // Base sample with fields common to ALL expected samples
    let base_sample = ScubaAccessLogSampleBuilder::new()
        .with_repo_id(RepositoryId::new(0))
        .with_client_identities(
            vec!["SERVICE_IDENTITY:myservice0"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
        )
        .with_client_main_id("service_identity:myservice0".to_string());

    RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_client_identity("SERVICE_IDENTITY:myservice0")?
        .with_tooling_allowlist_group("tooling_group")
        .with_test_groups(vec![("tooling_group", vec!["SERVICE_IDENTITY:myservice0"])])
        .with_test_repo_region_acls(vec![("restricted_acl", vec!["other_user"])])
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
            RestrictedPathManifestIdEntry::new(
                ManifestType::Fsnode,
                expected_fsnode_id.clone(),
                RepoPath::dir("restricted/dir")?,
            )?,
        ])
        .expecting_scuba_access_logs(vec![
            // HgManifest access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_manifest_id(expected_manifest_id.clone())
                .with_manifest_type(ManifestType::Hg)
                // Client HAS authorization because they are in the tooling allowlist
                .with_has_authorization(true)
                // Client IS in the tooling allowlist
                .with_is_allowlisted_tooling(true)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // HgAugmentedManifest access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_manifest_id(expected_manifest_id.clone())
                .with_manifest_type(ManifestType::HgAugmented)
                .with_has_authorization(true)
                .with_is_allowlisted_tooling(true)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Path access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir")?)
                .with_has_authorization(true)
                .with_is_allowlisted_tooling(true)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Fsnode access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_manifest_id(expected_fsnode_id.clone())
                .with_manifest_type(ManifestType::Fsnode)
                .with_has_authorization(true)
                .with_is_allowlisted_tooling(true)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Path fsnode access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir/a")?)
                .with_has_authorization(true)
                .with_is_allowlisted_tooling(true)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir/a")?)
                .with_has_authorization(true)
                .with_is_allowlisted_tooling(true)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
        ])
        .build(fb)
        .await?
        .run_restricted_paths_test()
        .await?;

    Ok(())
}

// Test that is_allowlisted_tooling is set to false when the client is NOT in the
// tooling allowlist group.
#[mononoke::fbinit_test]
async fn test_tooling_allowlist_acl_user_not_in_acl(fb: FacebookInit) -> Result<()> {
    // Service myservice0 does NOT have access to the tooling_group (only other_service does)
    let restricted_acl = MononokeIdentity::from_str("REPO_REGION:restricted_acl")?;
    let restricted_paths = vec![(
        NonRootMPath::new("restricted/dir").unwrap(),
        restricted_acl.clone(),
    )];

    let expected_manifest_id = ManifestId::from("0e3837eaab4fb0454c78f290aeb747a201ccd05b");
    let expected_fsnode_id =
        ManifestId::from("537548f0637858f6ebbba3e7f6c4d0c4e1ee7f88ca50fe3acff964115de0a0a3");

    // Base sample with fields common to ALL expected samples
    let base_sample = ScubaAccessLogSampleBuilder::new()
        .with_repo_id(RepositoryId::new(0))
        .with_client_identities(
            vec!["SERVICE_IDENTITY:myservice0"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
        )
        .with_client_main_id("service_identity:myservice0".to_string());

    RestrictedPathsTestDataBuilder::new()
        .with_restricted_paths(restricted_paths)
        .with_client_identity("SERVICE_IDENTITY:myservice0")?
        .with_tooling_allowlist_group("tooling_group")
        // myservice0 is NOT in the tooling_group
        .with_test_groups(vec![(
            "tooling_group",
            vec!["SERVICE_IDENTITY:other_service"],
        )])
        .with_test_repo_region_acls(vec![("restricted_acl", vec!["other_user"])])
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
            RestrictedPathManifestIdEntry::new(
                ManifestType::Fsnode,
                expected_fsnode_id.clone(),
                RepoPath::dir("restricted/dir")?,
            )?,
        ])
        .expecting_scuba_access_logs(vec![
            // HgManifest access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_manifest_id(expected_manifest_id.clone())
                .with_manifest_type(ManifestType::Hg)
                // Client does NOT have authorization to the restricted path
                .with_has_authorization(false)
                // Client is NOT in the tooling allowlist
                .with_is_allowlisted_tooling(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // HgAugmentedManifest access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_manifest_id(expected_manifest_id.clone())
                .with_manifest_type(ManifestType::HgAugmented)
                .with_has_authorization(false)
                .with_is_allowlisted_tooling(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Path access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir")?)
                .with_has_authorization(false)
                .with_is_allowlisted_tooling(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Fsnode access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_manifest_id(expected_fsnode_id.clone())
                .with_manifest_type(ManifestType::Fsnode)
                .with_has_authorization(false)
                .with_is_allowlisted_tooling(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            // Path fsnode access log
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir/a")?)
                .with_has_authorization(false)
                .with_is_allowlisted_tooling(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
            base_sample
                .clone()
                .with_restricted_paths(cast_to_non_root_mpaths(vec!["restricted/dir"]))
                .with_full_path(NonRootMPath::new("restricted/dir/a")?)
                .with_has_authorization(false)
                .with_is_allowlisted_tooling(false)
                .with_acls(vec![restricted_acl.clone()])
                .build()?,
        ])
        .build(fb)
        .await?
        .run_restricted_paths_test()
        .await?;

    Ok(())
}

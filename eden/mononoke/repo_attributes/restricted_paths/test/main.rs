/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use blobstore::Loadable;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use clientinfo::ClientRequestInfo;
use context::CoreContext;
use context::SessionContainer;
use fbinit::FacebookInit;
use futures::TryStreamExt;
use itertools::Itertools;
use manifest::ManifestOps;
use mercurial_derivation::derive_hg_changeset::DeriveHgChangeset;
use metaconfig_types::RestrictedPathsConfig;
use metadata::Metadata;
use mononoke_api::Repo as TestRepo;
use mononoke_api::RepoContext;
use mononoke_api_hg::HgTreeContext;
use mononoke_api_hg::RepoContextHgExt;
use mononoke_macros::mononoke;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use permission_checker::InternalAclProvider;
use permission_checker::MononokeIdentity;
use repo_blobstore::RepoBlobstoreRef;
use restricted_paths::SqlRestrictedPathsManifestIdStoreBuilder;
use restricted_paths::*;
use sql_construct::SqlConstruct;
use test_repo_factory::TestRepoFactory;
use tests_utils::CreateCommitContext;

struct RestrictedPathsTestData {
    ctx: CoreContext,
    repo: TestRepo,
    log_file_path: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
struct ScubaAccessLogSample {
    repo_id: RepositoryId,
    restricted_paths: Vec<NonRootMPath>,
    manifest_id: ManifestId,
    manifest_type: ManifestType,
    client_identities: Vec<String>,
    client_main_id: String,
    has_authorization: bool,
}

const TEST_CLIENT_MAIN_ID: &str = "user:myusername0";

#[mononoke::fbinit_test]
async fn test_mercurial_manifest_no_restricted_change(fb: FacebookInit) -> Result<()> {
    let restricted_paths = vec![(
        NonRootMPath::new("restricted/dir").unwrap(),
        MononokeIdentity::from_str("REPO_REGION:restricted_acl")?,
    )];
    let RestrictedPathsTestData {
        ctx,
        repo,
        log_file_path,
    } = setup_restricted_paths_test(fb, restricted_paths, None).await?;

    let (manifest_id_store_entries, scuba_logs) = hg_manifest_test_with_restricted_paths(
        &ctx,
        repo,
        vec![("unrestricted/dir/a", None)],
        &log_file_path,
    )
    .await?;

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
    let RestrictedPathsTestData {
        ctx,
        repo,
        log_file_path,
    } = setup_restricted_paths_test(fb, restricted_paths, None).await?;

    let (manifest_id_store_entries, scuba_logs) = hg_manifest_test_with_restricted_paths(
        &ctx,
        repo,
        vec![("user_project/foo/bar/a", None)],
        &log_file_path,
    )
    .await?;

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
    let RestrictedPathsTestData {
        ctx,
        repo,
        log_file_path,
    } = setup_restricted_paths_test(fb, restricted_paths, None).await?;

    let (manifest_id_store_entries, scuba_logs) = hg_manifest_test_with_restricted_paths(
        &ctx,
        repo,
        vec![("restricted/dir/a", None)],
        &log_file_path,
    )
    .await?;

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
    let RestrictedPathsTestData {
        ctx,
        repo,
        log_file_path,
    } = setup_restricted_paths_test(fb, restricted_paths, None).await?;

    let (manifest_id_store_entries, scuba_logs) = hg_manifest_test_with_restricted_paths(
        &ctx,
        repo,
        vec![("restricted/dir/a", None), ("restricted/dir/b", None)],
        &log_file_path,
    )
    .await?;

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
                manifest_id: expected_manifest_id,
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
    let RestrictedPathsTestData {
        ctx,
        repo,
        log_file_path,
    } = setup_restricted_paths_test(fb, restricted_paths, None).await?;

    let (manifest_id_store_entries, scuba_logs) = hg_manifest_test_with_restricted_paths(
        &ctx,
        repo,
        vec![("restricted/dir/a", None), ("unrestricted/dir/b", None)],
        &log_file_path,
    )
    .await?;

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
    let RestrictedPathsTestData {
        ctx,
        repo,
        log_file_path,
    } = setup_restricted_paths_test(fb, restricted_paths, None).await?;

    let (manifest_id_store_entries, scuba_logs) = hg_manifest_test_with_restricted_paths(
        &ctx,
        repo,
        vec![("restricted/one/a", None), ("restricted/two/b", None)],
        &log_file_path,
    )
    .await?;

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

// TODO(T239041722): find a way to test access to the restricted paths **with
// the proper authorization**, i.e. `has_authorization` should be logged as
// `false`.

// TODO(T239041722): test overlapping restricted directories. Top-level ACL should
// be enforced.

// TODO(T239041722): test different paths with same manifest id, where one
// is restricted and the other isn't.

//
// ----------------------------------------------------------------
// Test helpers

/// Given a list of restricted paths and a list of file paths, create a changeset
/// modifying those paths, derive the hg manifest and return all the entries
/// in the manifest id store.
/// Each file path can optionally specify content. If no content is provided,
/// the file path itself is used as the content.
async fn hg_manifest_test_with_restricted_paths(
    ctx: &CoreContext,
    repo: TestRepo,
    file_path_changes: Vec<(&str, Option<&str>)>,
    log_file_path: &std::path::Path,
) -> Result<(
    Vec<RestrictedPathManifestIdEntry>,
    Vec<ScubaAccessLogSample>,
)> {
    let mut commit_ctx = CreateCommitContext::new_root(ctx, &repo);
    for (path, content) in &file_path_changes {
        let file_content = content.unwrap_or(path);
        commit_ctx = commit_ctx.add_file(*path, file_content.to_string());
    }

    let bcs_id = commit_ctx.commit().await?;

    // Get the hg changeset id for the commit, to trigger hg manifest derivation
    let hg_cs_id = repo.derive_hg_changeset(ctx, bcs_id).await?;

    // Get all entries in the manifest id store
    let manifest_id_store_entries = repo
        .restricted_paths()
        .manifest_id_store()
        .get_all_entries(ctx)
        .await?;

    println!(
        "manifest_id_store_entries: {:#?}",
        manifest_id_store_entries
    );

    let repo = Arc::new(repo);
    let repo_ctx = RepoContext::new_test(ctx.clone(), repo.clone()).await?;
    let hg_repo_ctx = repo_ctx.hg();

    let _files_added = file_path_changes
        .into_iter()
        .map(|(path, _content)| NonRootMPath::new(path))
        .collect::<Result<Vec<_>>>()?;

    // Derive hg changeset to add entry for restricted paths
    let hg_cs = hg_cs_id.load(ctx, repo.repo_blobstore()).await?;
    let blobstore = Arc::new(repo.repo_blobstore().clone());

    // Then get the root manifest id, get all the trees and run
    // `HgTreeContext::new_check_exists` to simulate a directory access
    let hg_manif_id = hg_cs.manifestid();
    let _all_directories = hg_manif_id
        .list_tree_entries(ctx.clone(), blobstore.clone())
        .and_then(async |(path, hg_manifest_id)| {
            HgTreeContext::new_check_exists(hg_repo_ctx.clone(), hg_manifest_id).await?;

            Ok(path)
        })
        .try_collect::<Vec<_>>()
        .await?;

    let scuba_logs = deserialize_scuba_log_file(log_file_path)?;

    println!("scuba_logs: {scuba_logs:#?}");

    Ok((manifest_id_store_entries, scuba_logs))
}

/// Sets up an ACL file that will be used to create an ACL checker.
/// The ACL provides the test user access to all repos and
fn setup_acl_file(acl_json: Option<&str>) -> Result<std::path::PathBuf> {
    use std::io::Write;

    let mut temp_file = tempfile::NamedTempFile::new()?;
    let acl_content = acl_json.unwrap_or(
        r#"{
  "repos": {
    "default": {
      "actions": {
        "read": ["USER:myusername0"],
        "write": ["USER:myusername0"]
      }
    }
  },
  "repo_regions": {
    "myusername_project": {
      "actions": {
        "read": ["USER:myusername0"]
      }
    },
    "restricted_acl": {
      "actions": {
        "read": ["USER:another_user"]
      }
    }
  }
}"#,
    );

    temp_file.write_all(acl_content.as_bytes())?;
    temp_file.flush()?;
    let acl_path = temp_file.into_temp_path().keep()?;
    Ok(acl_path.to_path_buf())
}

async fn setup_test_repo(
    ctx: &CoreContext,
    restricted_paths: Vec<(NonRootMPath, MononokeIdentity)>,
    acl_json: Option<&str>,
) -> Result<TestRepo> {
    let repo_id = RepositoryId::new(0);
    let acl_file = setup_acl_file(acl_json)?;

    let acl_provider = InternalAclProvider::from_file(&acl_file)
        .with_context(|| format!("Failed to load ACLs from '{}'", acl_file.to_string_lossy()))?;

    let path_acls = restricted_paths.into_iter().collect();

    let manifest_id_store = Arc::new(
        SqlRestrictedPathsManifestIdStoreBuilder::with_sqlite_in_memory()
            .expect("Failed to create Sqlite connection")
            .with_repo_id(repo_id),
    );

    let config = RestrictedPathsConfig { path_acls };
    let repo_restricted_paths = Arc::new(RestrictedPaths::new(
        config,
        manifest_id_store,
        acl_provider,
    ));

    // Create the test repo
    let mut factory = TestRepoFactory::new(ctx.fb)?;
    let repo = factory
        .with_restricted_paths(repo_restricted_paths.clone())
        .build()
        .await?;
    Ok(repo)
}

async fn setup_restricted_paths_test(
    fb: FacebookInit,
    restricted_paths: Vec<(NonRootMPath, MononokeIdentity)>,
    acl_json: Option<&str>,
) -> Result<RestrictedPathsTestData> {
    let mut cri = ClientRequestInfo::new(ClientEntryPoint::Tests);
    cri.set_main_id(TEST_CLIENT_MAIN_ID.to_string());
    let client_info = ClientInfo::new_with_client_request_info(cri);

    let identities = BTreeSet::from([MononokeIdentity::new("USER", "myusername0")]);
    let metadata = {
        let mut md = Metadata::new(
            Some(&"restricted_paths_test".to_string()),
            identities,
            false,
            false,
            None,
            None,
        )
        .await;
        md.add_client_info(client_info);
        md
    };
    let session_container = SessionContainer::builder(fb)
        .metadata(Arc::new(metadata))
        .build();
    let ctx = CoreContext::test_mock_session(session_container);
    let repo = setup_test_repo(&ctx, restricted_paths, acl_json).await?;

    let temp_file = tempfile::NamedTempFile::new()?;
    let log_file_path = temp_file.path().to_path_buf();

    unsafe {
        std::env::set_var("ACCESS_LOG_SCUBA_FILE_PATH", &log_file_path);
    }

    Ok(RestrictedPathsTestData {
        ctx,
        repo,
        log_file_path,
    })
}

/// Reads the scuba log file and parses all samples as ScubaAccessLogSample
fn deserialize_scuba_log_file(
    scuba_log_file: &std::path::Path,
) -> Result<Vec<ScubaAccessLogSample>> {
    use std::fs::File;
    use std::io::BufRead;
    use std::io::BufReader;

    let file = match File::open(scuba_log_file) {
        Ok(file) => file,
        // If nothing is logged, file won't be created
        Err(_e) => return Ok(vec![]),
    };
    let reader = BufReader::new(file);

    // Collect all lines first (not efficient for very large files, but works for test logs)
    let lines: Vec<String> = reader.lines().collect::<std::io::Result<_>>()?;

    // Parse each line as a ScubaTelemetryLogSample object
    let log_samples: Vec<ScubaAccessLogSample> = lines
        .into_iter()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(&line)
                .map_err(anyhow::Error::from)
                .map(|json| {
                    // Scuba groups the logs by type (e.g. int, normal), so
                    // let's remove those and flatten the sample into a single
                    // json object.
                    let flattened_log =
                        json.as_object()
                            .iter()
                            .flat_map(|obj| {
                                obj.iter().flat_map(|(_, category_values)| {
                                    category_values.as_object().into_iter().flat_map(
                                        |category_obj| {
                                            category_obj.iter().map(|(k, v)| (k.clone(), v.clone()))
                                        },
                                    )
                                })
                            })
                            .collect::<serde_json::Value>();

                    println!("flattened_log: {flattened_log:#?}");

                    let repo_id: RepositoryId = flattened_log["repo_id"]
                        .as_number()
                        .and_then(|s| s.as_i64())
                        .and_then(|i| i.try_into().ok())
                        .map(RepositoryId::new)
                        .ok_or(anyhow!("missing repo_id"))?;

                    let client_main_id: String = flattened_log["client_main_id"]
                        .as_str()
                        .map(String::from)
                        .ok_or(anyhow!("missing client_main_id"))?;

                    let manifest_id: ManifestId = flattened_log["manifest_id"]
                        .as_str()
                        .map(String::from)
                        .map(ManifestId::from)
                        .ok_or(anyhow!("missing manifest_id"))?;

                    let manifest_type: ManifestType = flattened_log["manifest_type"]
                        .as_str()
                        .map(ManifestType::from_str)
                        .transpose()?
                        .ok_or(anyhow!("missing manifest_type"))?;

                    let has_authorization: bool = flattened_log["has_authorization"]
                        .as_str()
                        .map(|st| st.parse::<bool>())
                        .transpose()?
                        .ok_or(anyhow!("missing has_authorization"))?;

                    let restricted_paths: Vec<NonRootMPath> = flattened_log["restricted_paths"]
                        .as_array()
                        .map(|ids| {
                            ids.iter()
                                .filter_map(|id| id.as_str())
                                .sorted()
                                .map(NonRootMPath::new)
                                .collect::<Result<Vec<_>>>()
                        })
                        .transpose()?
                        .ok_or(anyhow!("missing restricted_paths"))?;

                    let client_identities: Vec<String> = flattened_log["client_identities"]
                        .as_array()
                        .map(|ids| {
                            ids.iter()
                                .filter_map(|id| id.as_str())
                                .map(String::from)
                                .sorted()
                                .collect()
                        })
                        .unwrap_or_default();

                    Ok(ScubaAccessLogSample {
                        repo_id,
                        restricted_paths,
                        manifest_id,
                        manifest_type,
                        client_identities,
                        has_authorization,
                        client_main_id,
                    })
                })?
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(log_samples)
}

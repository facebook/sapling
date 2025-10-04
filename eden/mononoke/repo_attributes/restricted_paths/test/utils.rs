/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;
use std::collections::HashMap;
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
use maplit::hashmap;
use mercurial_derivation::derive_hg_changeset::DeriveHgChangeset;
use metaconfig_types::RestrictedPathsConfig;
use metadata::Metadata;
use mononoke_api::Repo as TestRepo;
use mononoke_api::RepoContext;
use mononoke_api_hg::HgTreeContext;
use mononoke_api_hg::RepoContextHgExt;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use permission_checker::Acl;
use permission_checker::Acls;
use permission_checker::InternalAclProvider;
use permission_checker::MononokeIdentity;
use permission_checker::MononokeIdentitySet;
use pretty_assertions::assert_eq;
use repo_blobstore::RepoBlobstoreRef;
use restricted_paths::SqlRestrictedPathsManifestIdStoreBuilder;
use restricted_paths::*;
use sql_construct::SqlConstruct;
use test_repo_factory::TestRepoFactory;
use tests_utils::CreateCommitContext;

pub struct RestrictedPathsTestData {
    pub ctx: CoreContext,
    pub repo: TestRepo,
    pub log_file_path: std::path::PathBuf,
    // The changes that should be made in the test's commit. Each entry represents
    // a file to be created, along with its optional content. If no content is
    // provided, the file path itself is used as the content.
    pub file_path_changes: Vec<(String, Option<String>)>,
    // The entries you expect in the manifest id store after the test runs
    expected_manifest_entries: Option<Vec<RestrictedPathManifestIdEntry>>,
    // The scuba logs you expect to be logged after the test runs
    expected_scuba_logs: Option<Vec<ScubaAccessLogSample>>,
}

pub struct RestrictedPathsTestDataBuilder {
    restricted_paths: Vec<(NonRootMPath, MononokeIdentity)>,
    acls: Option<Acls>,
    file_path_changes: Vec<(String, Option<String>)>,
    expected_manifest_entries: Option<Vec<RestrictedPathManifestIdEntry>>,
    expected_scuba_logs: Option<Vec<ScubaAccessLogSample>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScubaAccessLogSample {
    pub repo_id: RepositoryId,
    pub restricted_paths: Vec<NonRootMPath>,
    pub manifest_id: Option<ManifestId>,
    pub manifest_type: Option<ManifestType>,
    pub client_identities: Vec<String>,
    pub client_main_id: String,
    pub has_authorization: bool,
}

pub const TEST_CLIENT_MAIN_ID: &str = "user:myusername0";

impl RestrictedPathsTestDataBuilder {
    pub fn new() -> Self {
        Self {
            restricted_paths: vec![],
            acls: None,
            file_path_changes: vec![],
            expected_manifest_entries: None,
            expected_scuba_logs: None,
        }
    }

    pub fn with_restricted_paths(
        mut self,
        restricted_paths: Vec<(NonRootMPath, MononokeIdentity)>,
    ) -> Self {
        self.restricted_paths = restricted_paths;
        self
    }

    pub fn with_test_acls(mut self, repo_regions_config: Vec<(&str, Vec<&str>)>) -> Self {
        self.acls = Some(setup_test_acls(repo_regions_config).expect("Failed to create test ACLs"));
        self
    }

    pub fn with_file_path_changes(mut self, file_path_changes: Vec<(&str, Option<&str>)>) -> Self {
        self.file_path_changes = file_path_changes
            .into_iter()
            .map(|(path, content)| (path.to_string(), content.map(|s| s.to_string())))
            .collect();
        self
    }

    // Set entries you expect in the manifest id store after the test runs
    pub fn expecting_manifest_id_store_entries(
        mut self,
        expected_manifest_entries: Vec<RestrictedPathManifestIdEntry>,
    ) -> Self {
        self.expected_manifest_entries = Some(expected_manifest_entries);
        self
    }

    // Set the scuba logs you expect to be logged after the test runs
    pub fn expecting_scuba_access_logs(
        mut self,
        expected_scuba_logs: Vec<ScubaAccessLogSample>,
    ) -> Self {
        self.expected_scuba_logs = Some(expected_scuba_logs);
        self
    }

    pub async fn build(self, fb: FacebookInit) -> Result<RestrictedPathsTestData> {
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
        let repo = setup_test_repo(&ctx, self.restricted_paths, self.acls).await?;

        let temp_file = tempfile::NamedTempFile::new()?;
        let log_file_path = temp_file.path().to_path_buf();

        unsafe {
            std::env::set_var("ACCESS_LOG_SCUBA_FILE_PATH", &log_file_path);
        }

        Ok(RestrictedPathsTestData {
            ctx,
            repo,
            log_file_path,
            file_path_changes: self.file_path_changes,
            expected_manifest_entries: self.expected_manifest_entries,
            expected_scuba_logs: self.expected_scuba_logs,
        })
    }
}

impl RestrictedPathsTestData {
    /// Given a list of restricted paths and a list of file paths, create a changeset
    /// modifying those paths, derive the hg manifest and fetch all the hg manifests
    /// from the last changeset, to simulate access to all directories in the repo.
    ///
    /// If expectations are set via the builder, this method will automatically verify
    /// them against the actual results. Otherwise, it will just run the test without
    /// any assertions.
    ///
    /// Each file path can optionally specify content. If no content is provided,
    /// the file path itself is used as the content.
    pub async fn run_restricted_paths_test(&self) -> Result<()> {
        let mut commit_ctx = CreateCommitContext::new_root(&self.ctx, &self.repo);
        for (path, content) in &self.file_path_changes {
            let file_content = content.as_deref().unwrap_or(path.as_str());
            commit_ctx = commit_ctx.add_file(path.as_str(), file_content.to_string());
        }

        let bcs_id = commit_ctx.commit().await?;

        // Get the hg changeset id for the commit, to trigger hg manifest derivation
        let hg_cs_id = self.repo.derive_hg_changeset(&self.ctx, bcs_id).await?;

        // Get all entries in the manifest id store
        let manifest_id_store_entries = self
            .repo
            .restricted_paths()
            .manifest_id_store()
            .get_all_entries(&self.ctx)
            .await?;

        println!(
            "manifest_id_store_entries: {:#?}",
            manifest_id_store_entries
        );

        let repo = Arc::new(self.repo.clone());
        let repo_ctx = RepoContext::new_test(self.ctx.clone(), repo.clone()).await?;
        let hg_repo_ctx = repo_ctx.hg();

        let _files_added = self
            .file_path_changes
            .iter()
            .map(|(path, _content)| NonRootMPath::new(path))
            .collect::<Result<Vec<_>>>()?;

        // Derive hg changeset to add entry for restricted paths
        let hg_cs = hg_cs_id.load(&self.ctx, self.repo.repo_blobstore()).await?;
        let blobstore = Arc::new(self.repo.repo_blobstore().clone());

        // Then get the root manifest id, get all the trees and run
        // `HgTreeContext::new_check_exists` to simulate a directory access
        let hg_manif_id = hg_cs.manifestid();
        let _all_directories = hg_manif_id
            .list_tree_entries(self.ctx.clone(), blobstore.clone())
            .and_then(async |(path, hg_manifest_id)| {
                HgTreeContext::new_check_exists(hg_repo_ctx.clone(), hg_manifest_id).await?;

                Ok(path)
            })
            .try_collect::<Vec<_>>()
            .await?;

        // Access is logged asynchronously, so wait for a bit before reading
        // the log file
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let scuba_logs = deserialize_scuba_log_file(&self.log_file_path)?;

        println!("scuba_logs: {scuba_logs:#?}");

        // Verify expectations if they were set
        if let Some(expected_manifest_entries) = &self.expected_manifest_entries {
            assert_eq!(manifest_id_store_entries, *expected_manifest_entries);
        }

        if let Some(expected_scuba_logs) = &self.expected_scuba_logs {
            assert_eq!(scuba_logs, *expected_scuba_logs);
        }

        Ok(())
    }
}

/// Creates an Acls structure for testing with specified repo regions and users.
/// The ACL provides the test user access to all repos and specified repo regions.
fn setup_test_acls(repo_regions_config: Vec<(&str, Vec<&str>)>) -> Result<Acls> {
    let mut repo_regions = HashMap::new();

    // Add each configured repo region
    for (region_name, usernames) in repo_regions_config {
        let mut users = MononokeIdentitySet::new();
        for username in usernames {
            users.insert(MononokeIdentity::from_str(&format!("USER:{}", username))?);
        }

        repo_regions.insert(
            region_name.to_string(),
            Arc::new(Acl {
                actions: hashmap! {
                    "read".to_string() => users,
                },
            }),
        );
    }

    let default_user = MononokeIdentity::from_str("USER:myusername0")?;
    let default_read_users = {
        let mut users = MononokeIdentitySet::new();
        users.insert(default_user.clone());
        users
    };
    let default_write_users = {
        let mut users = MononokeIdentitySet::new();
        users.insert(default_user);
        users
    };

    let repos = hashmap! {
        "default".to_string() => Arc::new(Acl {
            actions: hashmap! {
                "read".to_string() => default_read_users,
                "write".to_string() => default_write_users,
            },
        }),
    };

    Ok(Acls {
        repos,
        repo_regions,
        tiers: HashMap::new(),
        workspaces: HashMap::new(),
        groups: HashMap::new(),
    })
}

/// Creates a default Acls structure for testing.
/// The ACL provides the test user access to all repos and appropriate repo regions.
fn default_test_acls() -> Result<Acls> {
    setup_test_acls(vec![
        ("myusername_project", vec!["myusername0"]),
        ("restricted_acl", vec!["another_user"]),
    ])
}

/// Sets up an ACL file that will be used to create an ACL checker.
/// The ACL provides the test user access to all repos and appropriate repo regions.
fn setup_acl_file(acls: Option<Acls>) -> Result<std::path::PathBuf> {
    use std::io::Write;

    let mut temp_file = tempfile::NamedTempFile::new()?;

    let acls = acls.unwrap_or_else(|| default_test_acls().expect("Failed to create default ACLs"));
    let acl_content = serde_json::to_string_pretty(&acls)?;

    temp_file.write_all(acl_content.as_bytes())?;
    temp_file.flush()?;
    let acl_path = temp_file.into_temp_path().keep()?;
    Ok(acl_path.to_path_buf())
}

async fn setup_test_repo(
    ctx: &CoreContext,
    restricted_paths: Vec<(NonRootMPath, MononokeIdentity)>,
    acls: Option<Acls>,
) -> Result<TestRepo> {
    let repo_id = RepositoryId::new(0);
    let acl_file = setup_acl_file(acls)?;

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

                    let manifest_id = flattened_log["manifest_id"]
                        .as_str()
                        .map(String::from)
                        .map(ManifestId::from);

                    let manifest_type = flattened_log["manifest_type"]
                        .as_str()
                        .map(ManifestType::from_str)
                        .transpose()?;

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

pub(crate) fn cast_to_non_root_mpaths(paths: Vec<&str>) -> Vec<NonRootMPath> {
    paths
        .into_iter()
        .map(NonRootMPath::new)
        .collect::<Result<Vec<_>>>()
        .expect("Failed to cast to NonRootMPath")
}

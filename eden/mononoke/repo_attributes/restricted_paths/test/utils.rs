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
use fsnodes::RootFsnodeId;
use futures::TryStreamExt;
use itertools::Itertools;
use manifest::ManifestOps;
use maplit::hashmap;
use mercurial_derivation::RootHgAugmentedManifestId;
use mercurial_derivation::derive_hg_changeset::DeriveHgChangeset;
use mercurial_types::HgAugmentedManifestId;
use metaconfig_types::RestrictedPathsConfig;
use metadata::Metadata;
use mononoke_api::Repo as TestRepo;
use mononoke_api::RepoContext;
use mononoke_api_hg::HgDataId;
use mononoke_api_hg::RepoContextHgExt;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use permission_checker::Acl;
use permission_checker::Acls;
use permission_checker::InternalAclProvider;
use permission_checker::MononokeIdentity;
use permission_checker::MononokeIdentitySet;
use pretty_assertions::assert_eq;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use restricted_paths::SqlRestrictedPathsManifestIdStoreBuilder;
use restricted_paths::*;
use scuba_ext::MononokeScubaSampleBuilder;
use sql_construct::SqlConstruct;
use test_repo_factory::TestRepoFactory;
use tests_utils::CreateCommitContext;

pub const TEST_CLIENT_MAIN_ID: &str = "user:myusername0";

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
    repo_id: RepositoryId,
    restricted_paths: Vec<NonRootMPath>,
    manifest_id: Option<ManifestId>,
    manifest_type: Option<ManifestType>,
    full_path: Option<NonRootMPath>,
    client_identities: Vec<String>,
    client_main_id: String,
    has_authorization: bool,
    acls: Vec<MononokeIdentity>,
}

#[derive(Debug, Clone)]
pub struct ScubaAccessLogSampleBuilder {
    repo_id: Option<RepositoryId>,
    restricted_paths: Vec<NonRootMPath>,
    manifest_id: Option<ManifestId>,
    manifest_type: Option<ManifestType>,
    full_path: Option<NonRootMPath>,
    client_identities: Vec<String>,
    client_main_id: Option<String>,
    has_authorization: Option<bool>,
    acls: Vec<MononokeIdentity>,
}

impl ScubaAccessLogSampleBuilder {
    pub fn new() -> Self {
        Self {
            repo_id: None,
            restricted_paths: Vec::new(),
            manifest_id: None,
            manifest_type: None,
            full_path: None,
            client_identities: Vec::new(),
            client_main_id: None,
            has_authorization: None,
            acls: Vec::new(),
        }
    }

    pub fn with_repo_id(mut self, repo_id: RepositoryId) -> Self {
        self.repo_id = Some(repo_id);
        self
    }

    pub fn with_restricted_paths(mut self, restricted_paths: Vec<NonRootMPath>) -> Self {
        self.restricted_paths = restricted_paths;
        self
    }

    pub fn with_manifest_id(mut self, manifest_id: ManifestId) -> Self {
        self.manifest_id = Some(manifest_id);
        self
    }

    pub fn with_manifest_type(mut self, manifest_type: ManifestType) -> Self {
        self.manifest_type = Some(manifest_type);
        self
    }

    pub fn with_full_path(mut self, full_path: NonRootMPath) -> Self {
        self.full_path = Some(full_path);
        self
    }

    pub fn with_client_identities(mut self, client_identities: Vec<String>) -> Self {
        self.client_identities = client_identities;
        self
    }

    pub fn with_client_main_id(mut self, client_main_id: String) -> Self {
        self.client_main_id = Some(client_main_id);
        self
    }

    pub fn with_has_authorization(mut self, has_authorization: bool) -> Self {
        self.has_authorization = Some(has_authorization);
        self
    }

    pub fn with_acls(mut self, acls: Vec<MononokeIdentity>) -> Self {
        self.acls = acls;
        self
    }

    pub fn build(self) -> Result<ScubaAccessLogSample> {
        let repo_id = self.repo_id.ok_or_else(|| anyhow!("repo_id is required"))?;
        let client_main_id = self
            .client_main_id
            .ok_or_else(|| anyhow!("client_main_id is required"))?;
        let has_authorization = self
            .has_authorization
            .ok_or_else(|| anyhow!("has_authorization is required"))?;

        Ok(ScubaAccessLogSample {
            repo_id,
            restricted_paths: self.restricted_paths,
            manifest_id: self.manifest_id,
            manifest_type: self.manifest_type,
            full_path: self.full_path,
            client_identities: self.client_identities,
            client_main_id,
            has_authorization,
            acls: self.acls,
        })
    }
}

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

        let temp_file = tempfile::NamedTempFile::new()?;
        let log_file_path = temp_file.into_temp_path().keep()?;

        let repo = setup_test_repo(
            &ctx,
            self.restricted_paths,
            self.acls,
            log_file_path.clone(),
        )
        .await?;

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

        let blobstore = Arc::new(self.repo.repo_blobstore().clone());

        let bcs_id = commit_ctx.commit().await?;

        // Get the hg changeset id for the commit, to trigger hg manifest derivation
        let hg_cs_id = self.repo.derive_hg_changeset(&self.ctx, bcs_id).await?;

        let repo = Arc::new(self.repo.clone());
        let repo_ctx = RepoContext::new_test(self.ctx.clone(), repo.clone()).await?;
        let cs_ctx = repo_ctx
            .changeset(bcs_id)
            .await?
            .ok_or(anyhow!("failed to get changeset context"))?;
        let hg_repo_ctx = repo_ctx.clone().hg();

        let _files_added = self
            .file_path_changes
            .iter()
            .map(|(path, _content)| NonRootMPath::new(path))
            .collect::<Result<Vec<_>>>()?;

        // Derive hg changeset to add entry for restricted paths
        let hg_cs = hg_cs_id.load(&self.ctx, self.repo.repo_blobstore()).await?;

        // Then get the root manifest id, get all the trees and run
        // `HgTreeContext::new_check_exists` to simulate a directory access
        let hg_mfid = hg_cs.manifestid();

        // Derive HgAugmentedManifest
        let _root_hg_aug_manifest_id = self
            .repo
            .repo_derived_data()
            .derive::<RootHgAugmentedManifestId>(&self.ctx, bcs_id)
            .await?;

        // Derive Fsnode
        let root_fsnode_id = self
            .repo
            .repo_derived_data()
            .derive::<RootFsnodeId>(&self.ctx, bcs_id)
            .await?;

        // Sleep to ensure that the restricted paths cache was updated
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let _all_directories = hg_mfid
            // TODO(T239041722): list files as well to ensure access is logged when a file is requested
            .list_tree_entries(self.ctx.clone(), blobstore.clone())
            .and_then(async |(path, hg_manifest_id)| {
                // Access HgManifest
                let _tree_ctx = hg_manifest_id.context(hg_repo_ctx.clone()).await?;
                let hg_aug: HgAugmentedManifestId = hg_manifest_id.into();
                // Access HgAugmentedManifest
                let _aug_tree_ctx = hg_aug
                    .context(hg_repo_ctx.clone())
                    .await?
                    .ok_or(anyhow!("No HgAugmentedManifest for path {path:?}"))?;
                cs_ctx.path(path.clone()).await?;
                Ok(path)
            })
            .try_collect::<Vec<_>>()
            .await?;

        // Access all fsnode tree entries
        let fsnode_id = root_fsnode_id.into_fsnode_id();
        let _all_fsnode_directories = fsnode_id
            .list_tree_entries(self.ctx.clone(), blobstore.clone())
            .and_then(async |(path, fsnode_id)| {
                // Access Fsnode by loading it from blobstore
                let _tree = repo_ctx.tree(fsnode_id).await?;
                Ok(path)
            })
            .try_collect::<Vec<_>>()
            .await?;

        // Access path contents as we do in SCS for diffing
        let bonsai = cs_ctx.bonsai_changeset().await?;
        let paths = bonsai
            .file_changes_map()
            .keys()
            .map(|path| MPath::from(path.clone()))
            .collect::<BTreeSet<_>>();

        let _path_contexts = cs_ctx
            .paths_with_content(paths.into_iter())
            .await?
            .map_ok(|path_context| (path_context.path().clone(), path_context))
            .try_collect::<HashMap<_, _>>()
            .await?;

        // Get all entries in the manifest id store
        let manifest_id_store_entries = self
            .repo
            .restricted_paths()
            .manifest_id_store()
            .get_all_entries(&self.ctx)
            .await?;

        // Access is logged asynchronously, so wait for a bit before reading
        // the log file
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let scuba_logs = deserialize_scuba_log_file(&self.log_file_path)?;

        println!("scuba_logs: {scuba_logs:#?}");

        // Verify expectations if they were set
        if let Some(expected_manifest_entries) = self.expected_manifest_entries.clone() {
            assert_eq!(
                expected_manifest_entries
                    .into_iter()
                    .sorted()
                    .collect::<Vec<_>>(),
                manifest_id_store_entries
                    .into_iter()
                    .sorted()
                    .collect::<Vec<_>>()
            );
        }

        #[cfg(fbcode_build)]
        if let Some(expected_scuba_logs) = &self.expected_scuba_logs {
            assert_eq!(*expected_scuba_logs, scuba_logs);
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
    log_file_path: std::path::PathBuf,
) -> Result<TestRepo> {
    let repo_id = RepositoryId::new(0);
    let use_manifest_id_cache = true;
    let cache_update_interval_ms = 5;
    let acl_file = setup_acl_file(acls)?;

    let acl_provider = InternalAclProvider::from_file(&acl_file)
        .with_context(|| format!("Failed to load ACLs from '{}'", acl_file.to_string_lossy()))?;

    let path_acls = restricted_paths.into_iter().collect();

    let manifest_id_store = Arc::new(
        SqlRestrictedPathsManifestIdStoreBuilder::with_sqlite_in_memory()
            .expect("Failed to create Sqlite connection")
            .with_repo_id(repo_id),
    );

    let config = RestrictedPathsConfig {
        path_acls,
        use_manifest_id_cache,
        cache_update_interval_ms,
    };

    // Build the manifest id cache with the specified refresh interval
    let cache = Arc::new(
        RestrictedPathsManifestIdCacheBuilder::new(ctx.clone(), manifest_id_store.clone())
            .with_refresh_interval(std::time::Duration::from_millis(
                config.cache_update_interval_ms,
            ))
            .build()
            .await?,
    );

    // Create scuba builder that logs to the test file
    let scuba_builder = MononokeScubaSampleBuilder::with_discard().with_log_file(log_file_path)?;

    let repo_restricted_paths = Arc::new(RestrictedPaths::new(
        config,
        manifest_id_store.clone(),
        acl_provider,
        Some(cache),
        scuba_builder,
    ));

    // Create the test repo
    let mut factory = TestRepoFactory::new(ctx.fb)?;
    let repo = factory
        .with_restricted_paths(repo_restricted_paths)
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
                .with_context(|| format!("Failed to parse line: {}", line))
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

                    let full_path = flattened_log["full_path"]
                        .as_str()
                        .map(NonRootMPath::new)
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

                    let acls: Vec<MononokeIdentity> = flattened_log["acls"]
                        .as_array()
                        .map(|ids| {
                            let mut acls: Vec<MononokeIdentity> = ids
                                .iter()
                                .filter_map(|id| id.as_str())
                                .filter_map(|s| MononokeIdentity::from_str(s).ok())
                                .collect();
                            acls.sort();
                            acls
                        })
                        .unwrap_or_default();

                    Ok(ScubaAccessLogSample {
                        repo_id,
                        restricted_paths,
                        manifest_id,
                        manifest_type,
                        full_path,
                        client_identities,
                        has_authorization,
                        client_main_id,
                        acls,
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

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Contains structures describing configuration of the entire repo. Those structures are
//! deserialized from TOML files from metaconfig repo

#![deny(missing_docs)]

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::num::NonZeroU64;
use std::num::NonZeroUsize;
use std::ops::Deref;
use std::path::PathBuf;
use std::str;
use std::str::FromStr;
use std::time::Duration;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use ascii::AsciiString;
use bookmarks_types::BookmarkKey;
use derive_more::From;
use derive_more::Into;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use mononoke_types::NonRootMPath;
use mononoke_types::PrefixTrie;
use mononoke_types::RepositoryId;
use mononoke_types::hash::GitSha1;
use mononoke_types::path::MPath;
use mysql_common::value::convert::ConvIr;
use mysql_common::value::convert::FromValue;
use mysql_common::value::convert::ParseIr;
use regex::Regex;
use rusoto_core::Region;
use scuba::ScubaValue;
use serde_derive::Deserialize;
use sql::mysql;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;

/// A Regex that can be compared against other Regexes.
///
/// Regexes are compared using the string they were constructed from.  This is not
/// a semantic comparison, so Regexes that are functionally equivalent may compare
/// as different if they were constructed from different specifications.
#[derive(Debug, Clone)]
pub struct ComparableRegex(Regex);

impl ComparableRegex {
    /// Wrap a Regex so that it is comparable.
    pub fn new(regex: Regex) -> ComparableRegex {
        ComparableRegex(regex)
    }

    /// Extract the inner Regex from the wrapper.
    pub fn into_inner(self) -> Regex {
        self.0
    }
}

impl From<Regex> for ComparableRegex {
    fn from(regex: Regex) -> ComparableRegex {
        ComparableRegex(regex)
    }
}

impl Deref for ComparableRegex {
    type Target = Regex;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PartialEq for ComparableRegex {
    fn eq(&self, other: &Self) -> bool {
        self.as_str().eq(other.as_str())
    }
}

impl Eq for ComparableRegex {}

/// Structure representing general purpose identity.
#[derive(Clone, Default, Debug, Eq, PartialEq)]
pub struct Identity {
    /// Type of this identity.
    pub id_type: String,
    /// Associated data for this identity.
    pub id_data: String,
}

/// Configuration for how blobs are redacted
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RedactionConfig {
    /// Which blobstore should be used to fetch the redacted key lists
    pub blobstore: BlobConfig,
    /// Configerator location where RedactionSets object is stored
    pub redaction_sets_location: String,
}

/// Configuration for the async requests system
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AsyncRequestsConfig {
    /// The database used for the queue table
    pub db_config: Option<DatabaseConfig>,
    /// The blobstore used for request params and response
    pub blobstore: Option<BlobConfig>,
}

/// Configuration for all repos
#[facet::facet]
#[derive(Clone, Default, Debug, Eq, PartialEq)]
pub struct CommonConfig {
    /// Hipster tier that is permitted to act as a trusted proxy.
    pub trusted_parties_hipster_tier: Option<String>,
    /// Identities that act as trusted proxies.
    pub trusted_parties_allowlist: Vec<Identity>,
    /// Identities that are permitted to access all repos.
    pub global_allowlist: Vec<Identity>,
    /// Parent category to use for load limiting
    pub loadlimiter_category: Option<String>,
    /// Params for logging censored blobstore accesses
    pub censored_scuba_params: CensoredScubaParams,
    /// Whether to enable the control API over HTTP. At this time, this is only meant to be used in
    /// tests.
    pub enable_http_control_api: bool,
    /// Configuration for redaction of blobs
    pub redaction_config: RedactionConfig,
    /// Service identity for internal Mononoke services.
    pub internal_identity: Identity,
    /// Upper bound in bytes for the RSS memory that can be utilized by Mononoke GRit
    /// server for serving packfile stream
    pub git_memory_upper_bound: Option<u64>,
    /// Scuba table to dump edenapi requests to (for replay).
    pub edenapi_dumper_scuba_table: Option<String>,
    /// Configuration for the async requests system.
    pub async_requests_config: AsyncRequestsConfig,
}

/// Configuration for logging of censored blobstore accesses
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CensoredScubaParams {
    /// Scuba table for logging redacted file accesses
    pub table: Option<String>,
    /// Scuba table for logging redacted file accesses
    pub local_path: Option<String>,
}

/// Configuration of a single repository
#[facet::facet]
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct RepoConfig {
    /// If false, this repo config is completely ignored.
    pub enabled: bool,
    /// Persistent storage for this repo
    pub storage_config: StorageConfig,
    /// How large a cache to use (in bytes) for RepoGenCache derived information
    pub generation_cache_size: usize,
    /// Numerical repo id of the repo.
    pub repoid: RepositoryId,
    /// Scuba table for logging hook executions
    pub scuba_table_hooks: Option<String>,
    /// Local file to log hooks Scuba output to (useful in tests).
    pub scuba_local_path_hooks: Option<String>,
    /// Parameters of how to warm up the cache
    pub cache_warmup: Option<CacheWarmupParams>,
    /// Configuration for bookmarks
    pub bookmarks: Vec<BookmarkParams>,
    /// Infinitepush configuration
    pub infinitepush: InfinitepushParams,
    /// Configuration for hooks
    pub hooks: Vec<HookParams>,
    /// Push configuration options
    pub push: PushParams,
    /// Pushrebase configuration options
    pub pushrebase: PushrebaseParams,
    /// LFS configuration options
    pub lfs: LfsParams,
    /// What percent of read request verifies that returned content matches the hash
    pub hash_validation_percentage: usize,
    /// Should this repo reject write attempts
    pub readonly: RepoReadOnly,
    /// Should files be checked for redaction
    pub redaction: Redaction,
    /// Params for the hook manager
    pub hook_manager_params: Option<HookManagerParams>,
    /// Max number of results in listkeyspatterns.
    pub list_keys_patterns_max: u64,
    /// Params for File storage
    pub filestore: Option<FilestoreParams>,
    /// Maximum size to consider files in hooks
    pub hook_max_file_size: u64,
    /// Hipster ACL that controls access to this repo
    pub hipster_acl: Option<String>,
    /// Configuration for the Source Control Service
    pub source_control_service: SourceControlServiceParams,
    /// Configuration for Source Control Service monitoring
    pub source_control_service_monitoring: Option<SourceControlServiceMonitoring>,
    /// Derived data config for this repo
    pub derived_data_config: DerivedDataConfig,
    /// Whether to enforce strict LFS ACL checks for this repo.
    pub enforce_lfs_acl_check: bool,
    /// Whether to use warm bookmark cache while serving data hg wireprotocol
    pub repo_client_use_warm_bookmarks_cache: bool,
    /// Configuration for repo_client module
    pub repo_client_knobs: RepoClientKnobs,
    /// Callsign to check phabricator commits
    pub phabricator_callsign: Option<String>,
    /// ACL region configuration
    pub acl_region_config: Option<AclRegionConfig>,
    /// Walker configuration
    pub walker_config: Option<WalkerConfig>,
    /// Cross-repo commit validation config
    pub cross_repo_commit_validation_config: Option<CrossRepoCommitValidation>,
    /// Monitored spares profiles configuration.
    pub sparse_profiles_config: Option<SparseProfilesConfig>,
    /// Configuration for update logging.
    pub update_logging_config: UpdateLoggingConfig,
    /// Configuration for the commit graph.
    pub commit_graph_config: CommitGraphConfig,
    /// Default commit identity scheme. Some repos can be hg-mirrored git repos.
    pub default_commit_identity_scheme: CommitIdentityScheme,
    /// Config determining if the repo is deep sharded in the context of a service.
    /// shallow-sharded: Requests are sharded but repo is on every server
    /// deep-sharded: In addition to requests, repo is also sharded, i.e. present
    /// on select servers.
    pub deep_sharding_config: Option<ShardingModeConfig>,
    /// Local directory to write files to instead of uploading to everstore
    pub everstore_local_path: Option<String>,
    /// Configuration for the repo metadata logger
    pub metadata_logger_config: MetadataLoggerConfig,
    /// Configuration for connecting to Zelos
    pub zelos_config: Option<ZelosConfig>,
    /// The name of the bookmark used to compute repo size
    pub bookmark_name_for_objects_count: Option<String>,
    /// Default value for the objects count metric if it cannot be determined via TreeInfo.
    pub default_objects_count: Option<i64>,
    /// Overrides the value for the objects count metric for this repo, whether
    /// the actual value can be computed with TreeInfo or not (in fact, the computation is
    /// skipped entirely).
    pub override_objects_count: Option<i64>,
    /// Sets a multiplier for the value for the objects count metric for this repo
    pub objects_count_multiplier: Option<ObjectsCountMultiplier>,
    /// Map of XRepoSyncSourceConfig for the current repo keyed by the name of the target repo, e.g.
    /// XRepoSyncSourceConfig for the sync from whatsapp/server to fbsource will be stored as
    /// whatsapp_server_config.x_repo_sync_source_mapping["fbsource"] = config
    pub x_repo_sync_source_mapping: Option<XRepoSyncSourceConfigMapping>,
    /// Commit cloud configuration
    pub commit_cloud_config: CommitCloudConfig,
    /// Mononoke Cas Sync Configuration
    pub mononoke_cas_sync_config: Option<MononokeCasSyncConfig>,
    /// All Git related configs (e.g. Git Server and Git-only repos)
    pub git_configs: GitConfigs,
    /// Configuration for the modern sync job
    pub modern_sync_config: Option<ModernSyncConfig>,
    /// Expose continuous stats about repo contents, this is for when mononoke server is the receiving end of a sync
    pub log_repo_stats: bool,
    /// Configuration controlling the caching mechanism used for different types of Mononoke metadata
    /// (e.g. bookmarks, bonsai hash mappings, git symref mappings, etc.)
    pub metadata_cache_config: Option<MetadataCacheConfig>,
    /// Configuration whether we should advertise bundle-uri capability and serve bundle-uri
    /// endpoint.
    pub enable_git_bundle_uri: bool,
}

/// Config determining if the repo is deep sharded in the context of a service.
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct ShardingModeConfig {
    /// Deep sharded status of repo for individual services.
    pub status: HashMap<ShardedService, bool>,
}

/// Mononoke services for which sharding can be enabled.
#[derive(Eq, Clone, Debug, PartialEq, Hash)]
pub enum ShardedService {
    /// Eden / Mononoke Service
    SaplingRemoteApi,
    /// Source Control Service
    SourceControlService,
    /// Derived Data Service
    DerivedDataService,
    /// Source Control Land Service
    LandService,
    /// Derivation Worker (Worker for Derived Data Service)
    DerivationWorker,
    /// Large Files Service (LFS)
    LargeFilesService,
    /// Async Requests Worker
    AsyncRequestsWorker,
    /// Walker Scrub All
    WalkerScrubAll,
    /// Walker Validate All
    WalkerValidateAll,
    /// Derived Data Tailer
    DerivedDataTailer,
    /// Alias Verify
    AliasVerify,
    /// Draft Commit Deletion,
    DraftCommitDeletion,
    /// Mononoke Git Server
    MononokeGitServer,
    /// Repo Metadata Logger,
    RepoMetadataLogger,
    /// Modern Sync Job
    ModernSync,
    /// Bookmark Service
    BookmarkService,
    /// GitBundleGenerator
    GitBundleGenerator,
}

/// Indicates types of commit hashes used in a repo context.
#[derive(Eq, Clone, Debug, Default, PartialEq)]
pub enum CommitIdentityScheme {
    /// Hashes are Mercurial hashes.
    #[default]
    HG,
    /// Hashes are Git hashes.
    GIT,
    /// Hashes are Bonsai hashes.
    BONSAI,
    /// Hashes are of unknown scheme.
    UNKNOWN,
}

/// Configuration for repo_client module
#[derive(Eq, Copy, Clone, Default, Debug, PartialEq)]
pub struct RepoClientKnobs {
    /// Return shorter file history in getpack call
    pub allow_short_getpack_history: bool,
}

/// Config for derived data
#[derive(Eq, Clone, Default, Debug, PartialEq)]
pub struct DerivedDataConfig {
    /// Name of scuba table where all derivation will be logged to
    pub scuba_table: Option<String>,

    /// Name of configuration for enabled derived data types.
    pub enabled_config_name: String,

    /// All available configs for derived data types
    pub available_configs: HashMap<String, DerivedDataTypesConfig>,

    /// Name of scuba table to log all derivation queue operations
    pub derivation_queue_scuba_table: Option<String>,

    /// Config to use for remote derivation
    pub remote_derivation_config: Option<RemoteDerivationConfig>,

    /// Commits with blocked derivation
    pub blocked_derivation: HashMap<ChangesetId, Option<HashSet<DerivableType>>>,
}

impl DerivedDataConfig {
    /// Returns whether the named derived data type is enabled.
    pub fn is_enabled(&self, derivable_type: DerivableType) -> bool {
        if let Some(config) = self.available_configs.get(&self.enabled_config_name) {
            config.types.contains(&derivable_type)
        } else {
            false
        }
    }

    /// Return whether the named derived data type is enabled in named config
    pub fn is_enabled_for_config_name(
        &self,
        derivable_type: DerivableType,
        config_name: &str,
    ) -> bool {
        if let Some(config) = self.available_configs.get(config_name) {
            config.types.contains(&derivable_type)
        } else {
            false
        }
    }

    /// Returns active DerivedDataTypesConfig
    pub fn get_active_config(&self) -> Option<&DerivedDataTypesConfig> {
        self.available_configs.get(&self.enabled_config_name)
    }

    /// Returns mutable ref to active DerivedDataTypesConfig
    pub fn get_active_config_mut(&mut self) -> Option<&mut DerivedDataTypesConfig> {
        self.available_configs.get_mut(&self.enabled_config_name)
    }

    /// Returns DerivedDataTypesConfig for the given name from the list of available configs.
    pub fn get_config(&self, name: &str) -> Option<&DerivedDataTypesConfig> {
        self.available_configs.get(name)
    }

    /// Returns true if derivation is blocked for a commit and type.
    pub fn is_derivation_blocked(&self, derivable_type: DerivableType, csid: ChangesetId) -> bool {
        self.blocked_derivation
            .get(&csid)
            .is_some_and(|blocked_types| {
                blocked_types
                    .as_ref()
                    .is_none_or(|types| types.contains(&derivable_type))
            })
    }
}

/// Config for derived data types
#[derive(Eq, Clone, Default, Debug, PartialEq)]
pub struct DerivedDataTypesConfig {
    /// The configured types.
    pub types: HashSet<DerivableType>,

    /// Types that shouldn't be derived in ephemeral bubbles.
    pub ephemeral_bubbles_disabled_types: HashSet<DerivableType>,

    /// Key prefixes for mappings.  These are used to generate unique
    /// mapping keys when rederiving existing derived data types.
    ///
    /// Key prefixes only apply to derived data types where the mapping
    /// is stored in the blobstore.
    ///
    /// The prefix is applied to the commit hash part of the key, i.e.
    /// `derived_root_fsnode.HASH` becomes `derived_root_fsnode.PREFIXHASH`.
    pub mapping_key_prefixes: HashMap<DerivableType, String>,

    /// What unode version should be used.
    pub unode_version: UnodeVersion,

    /// Override the file size limit for blame. Blame won't be derived for files which
    /// size is above the limit. Default: `blame::DEFAULT_BLAME_FILESIZE_LIMIT`.
    pub blame_filesize_limit: Option<u64>,

    /// Whether to save committer field in commit extras when generating
    /// hg changesets.
    pub hg_set_committer_extra: bool,

    /// What blame version should be used.
    pub blame_version: BlameVersion,

    /// What `GitDeltaManifest` version should be used.
    pub git_delta_manifest_version: GitDeltaManifestVersion,

    /// Config for git delta manifest v2
    pub git_delta_manifest_v2_config: Option<GitDeltaManifestV2Config>,

    /// Config for git delta manifest v3
    pub git_delta_manifest_v3_config: Option<GitDeltaManifestV3Config>,

    /// For each Derived Data Type, what batch size should we use during derivation?
    pub derivation_batch_sizes: HashMap<DerivableType, usize>,

    /// Config for inferred copy from
    pub inferred_copy_from_config: Option<InferredCopyFromConfig>,
}

/// What type of unode derived data to generate
#[derive(Eq, Clone, Copy, Debug, Default, PartialEq)]
pub enum UnodeVersion {
    /// Unodes v2
    #[default]
    V2,
}

/// What type of blame derived data to generate
#[derive(Eq, Clone, Copy, Debug, Default, PartialEq)]
pub enum BlameVersion {
    /// Blame v2
    #[default]
    V2,
}

/// What `GitDeltaManifest` version should be used.
#[derive(Eq, Clone, Copy, Debug, Default, PartialEq)]
pub enum GitDeltaManifestVersion {
    #[default]
    /// GitDeltaManifest v2
    V2,
    /// GitDeltaManifest v3
    V3,
}

/// Config for git delta manifest v2
#[derive(Eq, Clone, Copy, Debug, Default, PartialEq)]
pub struct GitDeltaManifestV2Config {
    /// Maximum size allowed for an inlined full object.
    pub max_inlined_object_size: usize,
    /// Maximum size allowed for an inlined delta.
    pub max_inlined_delta_size: u64,
    /// Chunk size for delta instructions.
    pub delta_chunk_size: u64,
}

/// Config for git delta manifest v3
#[derive(Eq, Clone, Copy, Debug, Default, PartialEq)]
pub struct GitDeltaManifestV3Config {
    /// Maximum size allowed for an inlined full object.
    pub max_inlined_object_size: usize,
    /// Maximum size allowed for an inlined delta.
    pub max_inlined_delta_size: u64,
    /// Chunk size for delta instructions.
    pub delta_chunk_size: u64,
    /// Chunk size for git delta manifest entry.
    pub entry_chunk_size: usize,
}

/// Config for inferred copy from
#[derive(Eq, Clone, Copy, Debug, Default, PartialEq)]
pub struct InferredCopyFromConfig {
    /// When trying to find file copies using basename, how many levels of
    /// directories from the repo root should we perform the search?
    pub dir_level_for_basename_lookup: usize,
}

/// Config for remote derivation
#[derive(Eq, Clone, Debug, PartialEq)]
pub enum RemoteDerivationConfig {
    /// Shardmanager tier for remote derivation
    ShardManagerTier(String),
    /// SMC tier for remote derivation
    SmcTier(String),
    /// host:port string for remote derivation
    HostPort(String),
}

impl RepoConfig {
    /// Returns the address of the primary metadata database, or None if there is none.
    pub fn primary_metadata_db_address(&self) -> Option<&str> {
        self.storage_config.metadata.primary_address()
    }
}

#[derive(Eq, Copy, Clone, Debug, Default, PartialEq, Deserialize)]
/// Should the redaction verification be enabled?
pub enum Redaction {
    /// Redacted files cannot be accessed
    #[default]
    Enabled,
    /// All the files can be fetched
    Disabled,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
/// Is the repo read-only?
pub enum RepoReadOnly {
    /// This repo is read-only and should not accept pushes or other writes
    ReadOnly(String),
    /// This repo should accept writes.
    #[default]
    ReadWrite,
}

impl RepoReadOnly {
    /// Returns true if the repo is read-only
    pub fn is_read_only(&self) -> bool {
        match self {
            RepoReadOnly::ReadOnly(_) => true,
            RepoReadOnly::ReadWrite => false,
        }
    }
}

/// Configuration of warming up the Mononoke cache. This warmup happens on startup
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CacheWarmupParams {
    /// Bookmark to warmup cache for at the startup. If not set then the cache will be cold.
    pub bookmark: BookmarkKey,
    /// Max number to fetch during commit warmup. If not set in the config, then set to a default
    /// value.
    pub commit_limit: usize,
    /// Whether to use microwave to accelerate cache warmup.
    pub microwave_preload: bool,
}

/// Configuration for the hook manager
#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Default)]
pub struct HookManagerParams {
    /// Whether to disable the acl checker or not (intended for testing purposes)
    pub disable_acl_checker: bool,
    /// Whether to log admin bypasses.
    pub all_hooks_bypassed: bool,
    /// Scuba table for bypassed commits logging.
    pub bypassed_commits_scuba_table: Option<String>,
}

/// Configuration might be done for a single bookmark or for all bookmarks matching a regex
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BookmarkOrRegex {
    /// Matches a single bookmark
    Bookmark(BookmarkKey),
    /// Matches bookmarks with a regex
    Regex(ComparableRegex),
}

impl BookmarkOrRegex {
    /// Checks whether a given Bookmark matches this bookmark or regex
    pub fn matches(&self, bookmark: &BookmarkKey) -> bool {
        match self {
            BookmarkOrRegex::Bookmark(bm) => bm.eq(bookmark),
            BookmarkOrRegex::Regex(re) => re.is_match(&bookmark.to_string()),
        }
    }
}

impl From<BookmarkKey> for BookmarkOrRegex {
    fn from(b: BookmarkKey) -> Self {
        BookmarkOrRegex::Bookmark(b)
    }
}

impl From<Regex> for BookmarkOrRegex {
    fn from(r: Regex) -> Self {
        BookmarkOrRegex::Regex(ComparableRegex(r))
    }
}

/// Configuration for a bookmark
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BookmarkParams {
    /// The bookmark
    pub bookmark: BookmarkOrRegex,
    /// The hooks active for the bookmark
    pub hooks: Vec<String>,
    /// Are non fast forward moves blocked for this bookmark
    pub only_fast_forward: bool,
    /// Whether to rewrite dates for pushrebased commits or not
    pub rewrite_dates: Option<bool>,
    /// Only users matching this pattern or hipster group will be allowed to
    /// move this bookmark
    pub allowed_users: Option<ComparableRegex>,
    /// Only users matching this pattern or hipster group will be allowed to
    /// move this bookmark
    pub allowed_hipster_group: Option<String>,
    /// Skip hooks for changesets that are already ancestors of these
    /// bookmarks
    pub hooks_skip_ancestors_of: Vec<BookmarkKey>,
    /// Ensure that given bookmark(s) are ancestors of `ensure_ancestors_of`
    /// bookmark. That also implies that it's not longer possible to
    /// pushrebase to these bookmarks.
    pub ensure_ancestor_of: Option<BookmarkKey>,
    /// This option allows moving a bookmark to a commit that's already
    /// public while bypassing all the hooks. Note that should be fine,
    /// because commit is already public, meaning that hooks already
    /// should have been run when the commit was first made public.
    pub allow_move_to_public_commits_without_hooks: bool,
}

/// The type of the hook
#[derive(Debug, Clone, Eq, PartialEq, Deserialize)]
pub enum HookType {
    /// A hook that runs on the whole changeset
    PerChangeset,
    /// A hook that runs on a file in a changeset
    PerAddedOrModifiedFile,
}

impl FromStr for HookType {
    type Err = Error;

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        match string {
            "PerChangeset" => Ok(HookType::PerChangeset),
            "PerAddedOrModifiedFile" => Ok(HookType::PerAddedOrModifiedFile),
            _ => Err(anyhow!("Unable to parse {} as {}", string, "HookType")),
        }
    }
}

/// Hook bypass
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookBypass {
    /// Bypass that checks that a string is in the commit message
    commit_message_bypass: Option<String>,
    /// Bypass that checks that a string is in the commit message
    pushvar_name_and_value: Option<(String, String)>,
}

impl HookBypass {
    /// Create commit-message-only bypass
    pub fn new_with_commit_msg(msg: String) -> Self {
        Self {
            commit_message_bypass: Some(msg),
            pushvar_name_and_value: None,
        }
    }

    /// Create pushvar-only bypass
    pub fn new_with_pushvar(name: String, value: String) -> Self {
        Self {
            commit_message_bypass: None,
            pushvar_name_and_value: Some((name, value)),
        }
    }

    /// Create a bypass with both a commit message and a pushvar
    pub fn new_with_commit_msg_and_pushvar(
        msg: String,
        pushvar_name: String,
        pushvar_value: String,
    ) -> Self {
        Self {
            commit_message_bypass: Some(msg),
            pushvar_name_and_value: Some((pushvar_name, pushvar_value)),
        }
    }

    /// Get commit message bypass params
    pub fn commit_message_bypass(&self) -> Option<&String> {
        self.commit_message_bypass.as_ref()
    }

    /// Get pushvar bypass params
    pub fn pushvar_bypass(&self) -> Option<(&String, &String)> {
        self.pushvar_name_and_value
            .as_ref()
            .map(|name_and_value| (&name_and_value.0, &name_and_value.1))
    }
}

/// Configs that are being passed to the hook during runtime
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct HookConfig {
    /// An optional way to bypass a hook
    pub bypass: Option<HookBypass>,
    /// Configuration options (in JSON format)
    pub options: Option<String>,
    /// Whether this hook is log-only
    pub log_only: bool,

    // Deprecated config options
    /// Map of config to it's value. Values here are strings
    pub strings: HashMap<String, String>,
    /// **Warning:** this being deprecated, please use ints_64 instead. Map of config to it's value. Values here are 32bit integers
    pub ints: HashMap<String, i32>,
    /// Map of config to it's value. Values here are 64bit integers
    pub ints_64: HashMap<String, i64>,
    /// Map of config to it's value. Values here are lists of strings
    pub string_lists: HashMap<String, Vec<String>>,
    /// **Warning:** this being deprecated, please use int_64_lists. Map of config to it's value. Values here are lists of 32bit integers
    pub int_lists: HashMap<String, Vec<i32>>,
    /// Map of config to it's value. Values here are lists of 64bit integers
    pub int_64_lists: HashMap<String, Vec<i64>>,
}

impl HookConfig {
    /// Parse hook config options into a deserializable struct.
    pub fn parse_options<'a, T: serde::Deserialize<'a>>(&'a self) -> Result<T> {
        let options = self
            .options
            .as_ref()
            .ok_or_else(|| anyhow!("Missing hook options"))?;
        serde_json::from_str(options).context("Invalid hook config")
    }
}

/// Configuration for a hook
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookParams {
    /// The name of the hook
    pub name: String,
    /// The name of the hook implementation
    pub implementation: String,
    /// Configs that should be passed to hook
    pub config: HookConfig,
}

/// Push configuration options
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PushParams {
    /// Whether normal non-pushrebase pushes are allowed
    pub pure_push_allowed: bool,
    /// Limit of commits in a single unbundle
    pub unbundle_commit_limit: Option<u64>,
}

impl Default for PushParams {
    fn default() -> Self {
        PushParams {
            pure_push_allowed: true,
            unbundle_commit_limit: None,
        }
    }
}

/// Flags for the pushrebase inner loop
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PushrebaseFlags {
    /// Update dates of rebased commits
    pub rewritedates: bool,
    /// How far will we go from bookmark to find rebase root
    pub recursion_limit: Option<usize>,
    /// Forbid rebases when root is not a p1 of the rebase set.
    pub forbid_p2_root_rebases: bool,
    /// Whether to do chasefolding check during pushrebase
    pub casefolding_check: bool,
    /// Whether to do chasefolding check during pushrebase
    pub casefolding_check_excluded_paths: PrefixTrie,
    /// How many commits are allowed to not have filenodes generated.
    pub not_generated_filenodes_limit: u64,
    /// Which bookmark to track in ODS
    pub monitoring_bookmark: Option<String>,
}

impl Default for PushrebaseFlags {
    fn default() -> Self {
        PushrebaseFlags {
            rewritedates: true,
            recursion_limit: Some(16384), // this number is fairly arbitrary
            forbid_p2_root_rebases: true,
            casefolding_check: true,
            casefolding_check_excluded_paths: PrefixTrie::new(),
            not_generated_filenodes_limit: 500,
            monitoring_bookmark: None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
/// Either an SMC tier or a host/port pair
pub enum Address {
    /// An SMC tier
    Tier(String),
    /// A host:port string
    HostPort(String),
}

#[derive(Debug, Clone, Eq, PartialEq)]
/// How to do pushrebase on Mononoke
pub enum PushrebaseRemoteMode {
    /// Do pushrebase in the same process
    Local,
    /// Call LandService and do pushrebase remotely, forwarding errors
    RemoteLandService(Address),
    /// Call LandService and do pushrebase remotely, retrying errors locally
    RemoteLandServiceWithLocalFallback(Address),
}

/// Globalrev configuration
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GlobalrevConfig {
    /// On which bookmark to assign globalrevs
    pub publishing_bookmark: BookmarkKey,
    /// Present if this is a large repo and globalrevs go to a particular small repo.
    /// Contains the id of the small repo with globalrevs.
    pub globalrevs_small_repo_id: Option<RepositoryId>,
}

/// Pushrebase configuration options
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PushrebaseParams {
    /// Pushrebase processing flags
    pub flags: PushrebaseFlags,
    /// Block merge commits
    pub block_merges: bool,
    /// Whether to do emit obsmarkers after pushrebase
    pub emit_obsmarkers: bool,
    /// Globalrev configuration
    pub globalrev_config: Option<GlobalrevConfig>,
    /// Whether Git Mapping should be populated from extras (affects also blobimport)
    pub populate_git_mapping: bool,
    /// For the case when one repo is linked to another (a.k.a. megarepo)
    /// there's a special commit extra that allows changing the mapping
    /// used to rewrite a commit from one repo to another.
    /// Normally pushes of a commit like this are not allowed unless
    /// this option is set to false.
    pub allow_change_xrepo_mapping_extra: bool,
    /// How to do pushrebase on Mononoke
    pub remote_mode: PushrebaseRemoteMode,
}

impl Default for PushrebaseParams {
    fn default() -> Self {
        PushrebaseParams {
            flags: PushrebaseFlags::default(),
            block_merges: false,
            emit_obsmarkers: false,
            globalrev_config: None,
            populate_git_mapping: false,
            allow_change_xrepo_mapping_extra: false,
            remote_mode: PushrebaseRemoteMode::Local,
        }
    }
}

/// LFS configuration options
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct LfsParams {
    /// threshold in bytes, If None, Lfs is disabled
    pub threshold: Option<u64>,
    /// What percentage of clients should receive lfs pointers
    pub rollout_percentage: u32,
    /// Whether to use upstream LFS server
    pub use_upstream_lfs_server: bool,
}

/// Id used to discriminate different underlying blobstore instances
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Deserialize)]
#[derive(From, Into, mysql::OptTryFromRowField)]
pub struct BlobstoreId(u64);
sql::proxy_conv_ir!(BlobstoreId, ParseIr<u64>, u64);

impl BlobstoreId {
    /// Construct blobstore from integer
    pub fn new(id: u64) -> Self {
        BlobstoreId(id)
    }
}

impl fmt::Display for BlobstoreId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<BlobstoreId> for ScubaValue {
    fn from(blobstore_id: BlobstoreId) -> Self {
        ScubaValue::from(blobstore_id.0 as i64)
    }
}

/// Id used to identify storage configuration for a multiplexed blobstore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[derive(From, Into, mysql::OptTryFromRowField)]
pub struct MultiplexId(i32);
sql::proxy_conv_ir!(MultiplexId, ParseIr<i32>, i32);

impl MultiplexId {
    /// Construct a MultiplexId from an i32.
    pub fn new(id: i32) -> Self {
        Self(id)
    }
}

impl fmt::Display for MultiplexId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<MultiplexId> for ScubaValue {
    fn from(multiplex_id: MultiplexId) -> Self {
        ScubaValue::from(multiplex_id.0)
    }
}

/// Define storage needed for repo.
/// Storage consists of a blobstore and some kind of SQL DB for metadata. The configurations
/// can be broadly classified as "local" and "remote". "Local" is primarily for testing, and is
/// only suitable for single hosts. "Remote" is durable storage which can be shared by multiple
/// BlobRepo instances on different hosts.
#[derive(Debug, Default, Clone, Eq, PartialEq, Hash)]
pub struct StorageConfig {
    /// Blobstores. If the blobstore has a BlobstoreId then it can be used as a component of
    /// a Multiplexed blobstore.
    pub blobstore: BlobConfig,
    /// Metadata database
    pub metadata: MetadataDatabaseConfig,
    /// Blobstore for ephemeral changesets and snapshots.  If omitted
    /// then this repo cannot store ephemeral changesets or snapshots.
    pub ephemeral_blobstore: Option<EphemeralBlobstoreConfig>,
    /// Mutable blobstore, for storing mutable data like microwave
    pub mutable_blobstore: BlobConfig,
}

/// Whether we should read from this blobstore normally in a Multiplex,
/// or only read from it in Scrub or when it's our last chance to find the blob
#[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize, Hash)]
pub enum MultiplexedStoreType {
    /// Normal operation, no special treatment
    Normal,
    /// Writes go here as per normal, but it doesn't count towards quota, and is never read from.
    WriteOnly,
}

/// What format should data be in either Raw or a compressed form with compression options like level
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Deserialize, Hash)]
pub enum PackFormat {
    /// Uncompressed data is written by put
    #[default]
    Raw,
    /// Data will be compressed and written in compressed form if its smaller than Raw
    ZstdIndividual(i32),
}

/// Configuration for packblob
#[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize, Hash)]
pub struct PackConfig {
    /// What format should put write in, either Raw or a compressed form.
    pub put_format: PackFormat,
}

/// Configuration for a blobstore
#[derive(Debug, Clone, Default, Eq, PartialEq, Hash)]
pub enum BlobConfig {
    /// Administratively disabled blobstore
    #[default]
    Disabled,
    /// Blob repository with path pointing to on-disk files with data. Blobs are stored in
    /// separate files.
    /// NOTE: this is read-only and for development/testing only. Production uses will break things.
    Files {
        /// Path to directory containing files
        path: PathBuf,
    },
    /// Blob repository with path pointing to on-disk files with data. The files are stored in a
    /// Sqlite database
    Sqlite {
        /// Path to SQLite DB
        path: PathBuf,
    },
    /// Store in a manifold bucket
    Manifold {
        /// Bucket of the backing Manifold blobstore to connect to
        bucket: String,
        /// Prefix to be prepended to all the keys. In prod it should be ""
        prefix: String,
    },
    /// Store in a sharded Mysql
    Mysql {
        /// The remote database config
        remote: ShardableRemoteDatabaseConfig,
    },
    /// Multiplex across multiple blobstores for redundancy based on a WAL approach
    MultiplexedWal {
        /// A unique ID that identifies this multiplex configuration
        multiplex_id: MultiplexId,
        /// Set of blobstores being multiplexed over
        blobstores: Vec<(BlobstoreId, MultiplexedStoreType, BlobConfig)>,
        /// The number of writes that must succeed for the multiplex `put` to succeed
        write_quorum: usize,
        /// DB config to use for the WAL
        queue_db: ShardedDatabaseConfig,
        /// A scuba table to log stats per inner blobstore
        inner_blobstores_scuba_table: Option<String>,
        /// A scuba table to log status for the multiplexed blobstore
        multiplex_scuba_table: Option<String>,
        /// Used for both scuba tables. Write queries and read failures are not sampled.
        scuba_sample_rate: NonZeroU64,
    },
    /// Store in a manifold bucket, but every object will have an expiration
    ManifoldWithTtl {
        /// Bucket of the backing Manifold blobstore to connect to
        bucket: String,
        /// Prefix to be prepended to all the keys. In prod it should be ""
        prefix: String,
        /// TTL for each object we put in Manifold
        ttl: Duration,
    },
    /// A logging blobstore that wraps another blobstore
    Logging {
        /// The config for the blobstore that is wrapped.
        blobconfig: Box<BlobConfig>,
        /// The scuba table to log requests to.
        scuba_table: Option<String>,
        /// 1 in scuba_sample_rate samples will be logged.
        scuba_sample_rate: NonZeroU64,
    },
    /// An optionally-packing blobstore that wraps another blobstore
    Pack {
        /// The config for the blobstore that is wrapped.
        blobconfig: Box<BlobConfig>,
        /// Optional configuration for setting things like default compression levels
        pack_config: Option<PackConfig>,
    },
    /// Store in an AWS S3 bucket
    AwsS3 {
        /// Bucket to connect to
        bucket: String,
        /// AWS Region
        region: Region,
        /// Limit the number of concurrent operations to S3 blobstore.
        num_concurrent_operations: Option<usize>,
    },
}

impl BlobConfig {
    /// Return true if the blobstore is strictly local. Multiplexed blobstores are local iff
    /// all their components are.
    pub fn is_local(&self) -> bool {
        use BlobConfig::*;

        match self {
            Disabled | Files { .. } | Sqlite { .. } => true,
            Manifold { .. } | Mysql { .. } | ManifoldWithTtl { .. } | AwsS3 { .. } => false,
            MultiplexedWal { blobstores, .. } => blobstores
                .iter()
                .map(|(_, _, config)| config)
                .all(BlobConfig::is_local),
            Logging { blobconfig, .. } => blobconfig.is_local(),
            Pack { blobconfig, .. } => blobconfig.is_local(),
        }
    }

    /// If this blobstore performs sampling, update the sampling ratio.
    pub fn apply_sampling_multiplier(&mut self, multiplier: NonZeroU64) {
        match self {
            Self::MultiplexedWal {
                scuba_sample_rate, ..
            }
            | Self::Logging {
                scuba_sample_rate, ..
            } => {
                // NOTE: We unwrap here because we're multiplying two non zero numbers.
                *scuba_sample_rate =
                    NonZeroU64::new(scuba_sample_rate.get() * multiplier.get()).unwrap()
            }
            _ => {}
        }
    }
}

/// Configuration for a local SQLite database
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct LocalDatabaseConfig {
    /// Path to the directory containing the SQLite databases
    pub path: PathBuf,
}

/// Configuration for a remote MySQL database
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RemoteDatabaseConfig {
    /// SQL database to connect to
    pub db_address: String,
}

/// Configuration for a remote OSS MySQL database
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct OssRemoteDatabaseConfig {
    /// Host to connect to
    pub host: String,
    /// Port to connect to
    pub port: i16,
    /// Name of the database
    pub database: String,
    /// Name of the user secret
    pub user_secret: String,
    /// Name of the password secret
    pub password_secret: String,
}

/// Configuration for a sharded remote MySQL database
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ShardedRemoteDatabaseConfig {
    /// SQL database shard map to connect to
    pub shard_map: String,
    /// Number of shards to distribute data across.
    pub shard_num: NonZeroUsize,
}

/// Configuration for a potentially sharded remote MySQL database
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum ShardableRemoteDatabaseConfig {
    /// Database is not sharded.
    Unsharded(RemoteDatabaseConfig),
    /// Database is sharded.
    Sharded(ShardedRemoteDatabaseConfig),
}

/// Configuration for a single database
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum DatabaseConfig {
    /// Local SQLite database
    Local(LocalDatabaseConfig),
    /// Remote MySQL database
    Remote(RemoteDatabaseConfig),
}

impl DatabaseConfig {
    /// The address of this database, if this is a remote database.
    pub fn remote_address(&self) -> Option<String> {
        match self {
            Self::Remote(remote) => Some(remote.db_address.clone()),
            Self::Local(_) => None,
        }
    }
}

/// Configuration for a sharded database
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum ShardedDatabaseConfig {
    /// Local SQLite database
    Local(LocalDatabaseConfig),
    /// Remote MySQL sharded database
    Sharded(ShardedRemoteDatabaseConfig),
    /// Remote MySQL unsharded database (only for testing purposes)
    Unsharded(RemoteDatabaseConfig),
}

/// Configuration for the Metadata database when it is remote.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RemoteMetadataDatabaseConfig {
    /// Database for the primary metadata.
    pub primary: RemoteDatabaseConfig,
    /// Database for possibly sharded filenodes.
    pub filenodes: ShardableRemoteDatabaseConfig,
    /// Database for commit mutation metadata.
    pub mutation: RemoteDatabaseConfig,
    /// Database for sparse profiles sizes.
    pub sparse_profiles: RemoteDatabaseConfig,
    /// Database for bonsai blob mapping
    pub bonsai_blob_mapping: Option<ShardableRemoteDatabaseConfig>,
    /// Database for deletion log
    pub deletion_log: Option<RemoteDatabaseConfig>,
    /// Database for commit cloud info
    pub commit_cloud: Option<RemoteDatabaseConfig>,
    /// Database for commit git bundle uri metadata
    pub git_bundle_metadata: Option<RemoteDatabaseConfig>,
    /// Database for repo metadata logger checkpointing
    pub repo_metadata: Option<RemoteDatabaseConfig>,
}

/// Configuration for the Metadata database when it is remote.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct OssRemoteMetadataDatabaseConfig {
    /// Database for the primary metadata.
    pub primary: OssRemoteDatabaseConfig,
    /// Database for possibly sharded filenodes.
    pub filenodes: OssRemoteDatabaseConfig,
    /// Database for commit mutation metadata.
    pub mutation: OssRemoteDatabaseConfig,
    /// Database for sparse profiles sizes.
    pub sparse_profiles: OssRemoteDatabaseConfig,
    /// Database for bonsai blob mapping
    pub bonsai_blob_mapping: Option<OssRemoteDatabaseConfig>,
    /// Database for deletion log
    pub deletion_log: Option<OssRemoteDatabaseConfig>,
}

/// Configuration for the Metadata database
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum MetadataDatabaseConfig {
    /// Local SQLite database
    Local(LocalDatabaseConfig),
    /// Remote MySQL databases
    Remote(RemoteMetadataDatabaseConfig),
    /// OSS Remote MySQL Databases
    OssRemote(OssRemoteMetadataDatabaseConfig),
}

impl Default for MetadataDatabaseConfig {
    fn default() -> Self {
        MetadataDatabaseConfig::Local(LocalDatabaseConfig {
            path: PathBuf::default(),
        })
    }
}

impl MetadataDatabaseConfig {
    /// Whether this is a local on-disk database.
    pub fn is_local(&self) -> bool {
        match self {
            MetadataDatabaseConfig::Local(_) => true,
            MetadataDatabaseConfig::Remote(_) => false,
            MetadataDatabaseConfig::OssRemote(_) => false,
        }
    }

    /// The address of the primary metadata database, if this is a remote metadata database.
    pub fn primary_address(&self) -> Option<&str> {
        match self {
            MetadataDatabaseConfig::Remote(remote) => Some(&remote.primary.db_address),
            MetadataDatabaseConfig::OssRemote(_) => None,
            MetadataDatabaseConfig::Local(_) => None,
        }
    }
}

/// Enum configuration representing the possible modes
/// of deletion for expired bubbles.
#[derive(Debug, Copy, Clone, Default, Eq, PartialEq, Hash)]
pub enum BubbleDeletionMode {
    /// No marking or deletion
    #[default]
    Disabled,
    /// Mark bubbles as expired but don't delete them
    MarkOnly,
    /// Mark bubbles as expired and delete them
    MarkAndDelete,
}

/// Configuration for the ephemeral blobstore, which stores
/// blobs for ephemeral changesets and snapshots.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct EphemeralBlobstoreConfig {
    /// The configuration of the blobstore where ephemeral blobs
    /// are stored.
    pub blobstore: BlobConfig,

    /// Configuration of the database where metadata for the
    /// ephemeral blobstore (e.g. bubble expiration) is stored.
    pub metadata: DatabaseConfig,

    /// Initial lifespan for bubbles.
    pub initial_bubble_lifespan: Duration,

    /// Grace period for already-opened bubbles after expiration.
    pub bubble_expiration_grace: Duration,

    /// Mode deciding if the bubbles should be simply marked as
    /// expired or completely deleted from the backing store.
    pub bubble_deletion_mode: BubbleDeletionMode,
}

/// Regex for valid branches that Infinite Pushes can be directed to.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InfinitepushNamespace(ComparableRegex);

impl InfinitepushNamespace {
    /// Instantiate a new InfinitepushNamespace
    pub fn new(regex: Regex) -> Self {
        Self(ComparableRegex(regex))
    }

    /// Returns whether a given Bookmark matches this namespace.
    pub fn matches_bookmark(&self, bookmark: &BookmarkKey) -> bool {
        self.0.is_match(bookmark.as_str())
    }

    /// Returns this namespace's controlling Regex.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Infinitepush configuration. Note that it is legal to not allow Infinitepush (server = false),
/// while still providing a namespace. Doing so will prevent regular pushes to the namespace, as
/// well as allow the creation of Infinitepush scratchbookmarks through e.g. replicating them from
/// Mercurial.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct InfinitepushParams {
    /// Whether infinite push bundles are allowed on this server. If false, all infinitepush
    /// bundles will be rejected.
    pub allow_writes: bool,

    /// Valid namespace for infinite push bookmarks. If None, then infinitepush bookmarks are not
    /// allowed.
    pub namespace: Option<InfinitepushNamespace>,

    /// Whether to put trees/files in the getbundle response for infinitepush commits
    pub hydrate_getbundle_response: bool,
}

/// Filestore configuration.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FilestoreParams {
    /// Chunk size for the Filestore, in bytes.
    pub chunk_size: u64,
    /// Max number of concurrent chunk uploads to perform in the Filestore.
    pub concurrency: usize,
}

/// Default path action to perform when syncing commits
/// from small to large repos
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DefaultSmallToLargeCommitSyncPathAction {
    /// Preserve as is
    Preserve,
    /// Prepend a given prefix to the path
    PrependPrefix(NonRootMPath),
}

/// Whether any changes made to git submodules should be stripped from
/// the changesets before being synced.
/// Since this is used in the small repo config, defininig a struct to set the
/// default to true, to avoid accidentally syncing git submodules to large repos.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub enum GitSubmodulesChangesAction {
    /// Sync all changes made to git submodules without alterations.
    Keep,
    /// Strip any changes made to git submodules from the synced bonsai.
    #[default]
    Strip,
    /// Expand any submodule file change into multiple file changes that
    /// achieve the same working copy. i.e. Copy the contents of the submodule
    /// repo into the synced version of the source repo in the target repo.
    /// This requires the `submodule_dependencies` field to be properly set
    /// in the small repo's sync config.
    Expand,
}

/// Default prefix for git submodule metadata files
pub const DEFAULT_GIT_SUBMODULE_METADATA_FILE_PREFIX: &str = "x-repo-submodule";

/// Stores all the information related to git submodules in a small repo,
/// e.g. how to handle them and what other repos the small repo might depend on
/// to expand submodule file changes.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SmallRepoGitSubmoduleConfig {
    /// Whether any changes made to git submodules should be stripped from
    /// the changesets before being synced.
    pub git_submodules_action: GitSubmodulesChangesAction,
    /// Map from submodule path in the small repo to the ID of the submodule's
    /// repository in Mononoke.
    /// These repos have to be loaded with the small repo before syncing starts,
    /// as file changes from the submodule dependencies might need to be copied.
    pub submodule_dependencies: HashMap<NonRootMPath, RepositoryId>,
    /// Each submodule expansion in the large repo will have a metadata file
    /// named "<PREFIX><SUBMODULE_PATH>", e.g. ".x-repo-submodule-voip".
    /// This file will store the git commit that the expansion corresponds to.
    pub submodule_metadata_file_prefix: String,

    /// List git commit hashes that are known dangling submodule pointers in the
    /// repo's history, i.e. don't actually exist in the submodule repo it's
    /// supposed to point to.
    /// This can happen after non-fast-forward pushes or accidentally pushing
    /// commits with local submodule pointers.
    ///
    /// The expansion of these commits will contain a single text file informing
    /// that the expansion belongs to a dangling submodule pointer.
    pub dangling_submodule_pointers: Vec<GitSha1>,
}

impl Default for SmallRepoGitSubmoduleConfig {
    fn default() -> Self {
        Self {
            git_submodules_action: GitSubmodulesChangesAction::default(),
            submodule_dependencies: HashMap::new(),
            submodule_metadata_file_prefix: DEFAULT_GIT_SUBMODULE_METADATA_FILE_PREFIX.to_string(),
            dangling_submodule_pointers: Vec::new(),
        }
    }
}

/// Commit sync configuration for a small repo
/// Note: this configuration is always from the point of view
/// of the small repo, meaning a key in the `map` is a path
/// prefix in the small repo, and a value - in the large repo
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SmallRepoCommitSyncConfig {
    /// Default action to take on a path
    pub default_action: DefaultSmallToLargeCommitSyncPathAction,
    /// A map of prefix replacements when syncing
    pub map: HashMap<NonRootMPath, NonRootMPath>,
    /// All information related to git submodules
    pub submodule_config: SmallRepoGitSubmoduleConfig,
}

/// Commit sync direction
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CommitSyncDirection {
    /// Syncing commits from large repo to small ones
    Backwards,
    /// Syncing commits from small repos to large one
    Forward,
}

impl CommitSyncDirection {
    /// Return the opposite sync direction
    pub fn reverse(&self) -> Self {
        match self {
            Self::Backwards => Self::Forward,
            Self::Forward => Self::Backwards,
        }
    }
}

/// CommitSyncConfig version name
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive(mysql::OptTryFromRowField)]
#[derive(bincode::Encode, bincode::Decode)]
pub struct CommitSyncConfigVersion(pub String);

impl fmt::Display for CommitSyncConfigVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<CommitSyncConfigVersion> for Value {
    fn from(version: CommitSyncConfigVersion) -> Self {
        Value::Bytes(version.0.into_bytes())
    }
}

impl ConvIr<CommitSyncConfigVersion> for CommitSyncConfigVersion {
    fn new(v: Value) -> Result<Self, FromValueError> {
        match v {
            Value::Bytes(bytes) => match String::from_utf8(bytes) {
                Ok(s) => Ok(CommitSyncConfigVersion(s)),
                Err(from_utf8_error) => {
                    Err(FromValueError(Value::Bytes(from_utf8_error.into_bytes())))
                }
            },
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> CommitSyncConfigVersion {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for CommitSyncConfigVersion {
    type Intermediate = CommitSyncConfigVersion;
}

/// Commit sync configuration for a large repo
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommitSyncConfig {
    /// Large repository id
    pub large_repo_id: RepositoryId,
    /// Common pushrebase bookmarks
    pub common_pushrebase_bookmarks: Vec<BookmarkKey>,
    /// Corresponding small repo configs
    pub small_repos: HashMap<RepositoryId, SmallRepoCommitSyncConfig>,
    /// Version name of the commit sync config
    pub version_name: CommitSyncConfigVersion,
}

/// Config that applies to all mapping versions
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommonCommitSyncConfig {
    /// Large repository id
    pub large_repo_id: RepositoryId,
    /// Common pushrebase bookmarks
    pub common_pushrebase_bookmarks: Vec<BookmarkKey>,
    /// Small repos configs
    pub small_repos: HashMap<RepositoryId, SmallRepoPermanentConfig>,
}

/// Permanent config for a single small repo
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SmallRepoPermanentConfig {
    /// Prefix of the bookmark
    pub bookmark_prefix: AsciiString,
    /// Mapping from each common_pushrebase_bookmark in the large repo to
    /// the equivalent bookmark in the small repo.
    /// This allows using a different bookmark name for the common pushrebase bookmark
    /// between the large repos and some of the small repos (e.g: a small repo imported
    /// from git may want to sync its `heads/master` to `master` in a large repo)
    pub common_pushrebase_bookmarks_map: HashMap<BookmarkKey, BookmarkKey>,
}

/// Source Control Service options
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct SourceControlServiceParams {
    /// Whether writes are permitted.
    pub permit_writes: bool,

    /// Whether writes by services are permitted.
    pub permit_service_writes: bool,

    /// ACL name for determining permissions to act as a service.  If service
    /// writes are permitted (`permit_service_writes = true`) and this is
    /// `None`, then any client may act as any service.
    pub service_write_hipster_acl: Option<String>,

    /// Map from service identity to the restrictions that apply for that service
    pub service_write_restrictions: HashMap<String, ServiceWriteRestrictions>,

    /// Whether users can create commits without parents.
    pub permit_commits_without_parents: bool,
}

impl SourceControlServiceParams {
    /// Returns true if the named service is permitted to call the named method.
    pub fn service_write_method_permitted(
        &self,
        service_identity: impl AsRef<str>,
        method: impl AsRef<str>,
    ) -> bool {
        if let Some(restrictions) = self
            .service_write_restrictions
            .get(service_identity.as_ref())
        {
            return restrictions.permitted_methods.contains(method.as_ref());
        }
        false
    }

    /// Returns true if the named service is permitted to modify the named bookmark.
    pub fn service_write_bookmark_permitted(
        &self,
        service_identity: impl AsRef<str>,
        bookmark: &BookmarkKey,
    ) -> bool {
        if let Some(restrictions) = self
            .service_write_restrictions
            .get(service_identity.as_ref())
        {
            if restrictions.permitted_bookmarks.contains(bookmark.as_str()) {
                return true;
            }
            if let Some(regex) = &restrictions.permitted_bookmark_regex {
                if regex.is_match(bookmark.as_str()) {
                    return true;
                }
            }
        }
        false
    }

    /// Returns true if the named service is permitted to modify all paths.
    pub fn service_write_all_paths_permitted(&self, service_identity: impl AsRef<str>) -> bool {
        if let Some(restrictions) = self
            .service_write_restrictions
            .get(service_identity.as_ref())
        {
            return restrictions.permitted_path_prefixes.contains_everything();
        }
        false
    }

    /// Returns true if the named service is permitted to modify all of the paths
    /// that a bonsai changeset modifies.
    pub fn service_write_paths_permitted<'cs>(
        &self,
        service_identity: impl AsRef<str>,
        bonsai: &'cs BonsaiChangeset,
    ) -> Result<(), &'cs NonRootMPath> {
        if let Some(restrictions) = self
            .service_write_restrictions
            .get(service_identity.as_ref())
        {
            // Currently path prefixes are only used to grant permission.
            // This means we only need to check if all of the bonsai paths
            // are covered by the prefixes in the configuration.
            //
            // In the future, we may want to add exclusions to the paths
            // (e.g. dir1/ is permitted except for dir1/subdir1/).  When
            // this happens we'll need to do a manifest diff, as the bonsai
            // changes won't include dir1/subdir1/ files if dir1 is
            // replaced by a file.
            let trie = &restrictions.permitted_path_prefixes;
            for path in bonsai.file_changes_map().keys() {
                if !trie.contains_prefix(path) {
                    return Err(path);
                }
            }
        }
        Ok(())
    }
}

/// Restrictions on writes for services.
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct ServiceWriteRestrictions {
    /// The service is permissed to call these methods
    pub permitted_methods: HashSet<String>,

    /// The service is permitted to modify files with these path prefixes.
    pub permitted_path_prefixes: PrefixTrie,

    /// The service is permitted to modify these bookmarks.
    pub permitted_bookmarks: HashSet<String>,

    /// The service is permitted to modify bookmarks that match this regex in addition
    /// to those specified by `permitted_bookmarks`.
    pub permitted_bookmark_regex: Option<ComparableRegex>,
}

/// Configuration for health monitoring of the Source Control Service
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SourceControlServiceMonitoring {
    /// Bookmarks, for which we want our services to log
    /// age values to monitoring counters. For example,
    /// a freshness value may be the `now - author_date` of
    /// the commit, to which the bookmark points
    pub bookmarks_to_report_age: Vec<BookmarkKey>,
}

/// Define a region of the repository, in terms of commits and path prefixes.
///
/// The commit range is equivalent to the Mercurial revset
///     descendants(roots) - descendants(heads)
///
/// If the roots and heads lists are both empty then this region covers the
/// entire repo.
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct AclRegion {
    /// List of roots that begin this region.  Any commit that is a descendant of any
    /// root, including the root itself, will be included in the region.  If this
    /// list is empty then all commits are included (provided they are not the
    /// descendant of a head).
    pub roots: Vec<ChangesetId>,

    /// List of heads that end this region.  Any commit that is a descendant of
    /// any head, including the head itself, will NOT be included in the region.
    /// If this list is empty then all commits that are descendants of the roots
    /// are included.
    pub heads: Vec<ChangesetId>,

    /// List of path prefixes that apply to this region.  Prefixes are in terms of
    /// path elements, so the prefix a/b applies to a/b/c but not a/bb.
    pub path_prefixes: Vec<MPath>,
}

/// ACL region rule consisting of multiple regions and path prefixes
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct AclRegionRule {
    /// The name of this region rule.  This is used in error messages and diagnostics.
    pub name: String,
    /// A list of regions that this rule applies to.
    pub regions: Vec<AclRegion>,
    /// The hipster ACL that defines who is permitted to access the regions of
    /// the repo defined by this rule.
    pub hipster_acl: String,
}

/// Describe ACL Regions for a repository.
///
/// This is a set of rules which define regions of the repository (commits and paths)
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct AclRegionConfig {
    /// List of rules that grant access to regions of the repo.
    pub allow_rules: Vec<AclRegionRule>,
}

/// Walker parameters that are specific to type of job and repo.
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct WalkerJobParams {
    /// Controls max concurrency for MySQL and other dependencies
    pub scheduled_max_concurrency: Option<i64>,
    /// Controls the max blobstore read QPS for a given repo
    pub qps_limit: Option<i64>,
    /// The type of nodes to be excluded during walk
    pub exclude_node_type: Option<String>,
    /// Whether to allow remaining deferred edges after chunks complete.
    pub allow_remaining_deferred: bool,
    /// Control whether walker continues in the face of error for specified
    /// node types
    pub error_as_node_data_type: Option<String>,
}

#[derive(Debug, Copy, clap::ValueEnum, Clone, Eq, PartialEq, Hash)]
/// The type of walker jobs deployed in production
pub enum WalkerJobType {
    /// Invalid value
    Unknown,
    /// Validate Job
    ValidateAll,
    /// ScrubAllChunked Job
    ScrubAllChunked,
    /// ScrubHgAllChunked Job
    ScrubHgAllChunked,
    /// ScrubHgFileContent Job
    ScrubHgFileContent,
    /// ScrubHgFileNode Job
    ScrubHgFileNode,
    /// ScrubUnodeAllChunked Job
    ScrubUnodeAllChunked,
    /// ScrubUnodeBlame Job
    ScrubUnodeBlame,
    /// ScrubDerivedNoContentMeta Job
    ScrubDerivedNoContentMeta,
    /// ScrubDerivedNoContentMetaChunked Job
    ScrubDerivedNoContentMetaChunked,
    /// ScrubUnodeFastlog Job
    ScrubUnodeFastlog,
    /// ScrubDerivedChunked Job
    ScrubDerivedChunked,
    /// ShallowHgScrub Job
    ShallowHgScrub,
}

impl fmt::Display for WalkerJobType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let str_val = match &self {
            Self::Unknown => "unknown",
            Self::ScrubAllChunked => "scrub-all-chunked",
            Self::ScrubDerivedChunked => "scrub-derived-chunked",
            Self::ScrubDerivedNoContentMeta => "scrub-derived-no-content-meta",
            Self::ScrubDerivedNoContentMetaChunked => "scrub-derived-no-content-meta-chunked",
            Self::ScrubHgAllChunked => "scrub-hg-all-chunked",
            Self::ScrubHgFileContent => "scrub-hg-file-content",
            Self::ScrubHgFileNode => "scrub-hg-file-node",
            Self::ScrubUnodeAllChunked => "scrub-unode-all-chunked",
            Self::ScrubUnodeBlame => "scrub-unode-blame",
            Self::ScrubUnodeFastlog => "scrub-unode-fastlog",
            Self::ShallowHgScrub => "shallow-hg-scrub",
            Self::ValidateAll => "validate-all",
        };
        write!(f, "{}", str_val)
    }
}

/// Configuration relevant to walker job.
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct WalkerConfig {
    /// Determines if the walker should scrub blobs.
    pub scrub_enabled: bool,
    /// Determines if the walker should validate blobs.
    pub validate_enabled: bool,
    /// Parameters for different walker jobs.
    pub params: Option<HashMap<WalkerJobType, WalkerJobParams>>,
}

/// Configuration relevant to cross-repo commit validation
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct CrossRepoCommitValidation {
    /// A set of bookmarks whose changelog entries are deemed to be valid
    /// Commits that are only found via the changelog for this named bookmark
    /// are skipped for validation (e.g. import bookmarks can be skipped)
    pub skip_bookmarks: HashSet<BookmarkKey>,
}

/// Configuration for sparse profile monitoring
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct SparseProfilesConfig {
    /// Location where sparse profiles are stored within the repo
    pub sparse_profiles_location: String,
    /// Excluded paths and files from monitoring
    /// used as glob patterns for pathmatchers
    pub excluded_paths: Vec<String>,
    /// Exact list of monitored profiles
    /// Takes precedence over excludes.
    pub monitored_profiles: Vec<String>,
}

/// Repo-specific configuration parameters for mononoke cas sync job
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct MononokeCasSyncConfig {
    /// The name of the main bookmark to sync to RE CAS
    pub main_bookmark_to_sync: String,
    /// Enabling it would expand the sync to all the bookmarks
    pub sync_all_bookmarks: bool,
    /// CAS use case to use for uploading public commits
    pub use_case_public: String,
    /// CAS use case to use for uploading draft commits
    pub use_case_draft: String,
}

/// Repo-specific configuration parameters for modern sync job for a specific job variant
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ModernSyncConfig {
    /// Edenapi url to use for modern sync
    pub url: String,
    /// Size of each chunk from a big bookmark mve.
    pub chunk_size: i64,
    /// Maximum number of BUL entries to query from a single database transaction.
    pub single_db_query_entries_limit: i64,
    /// Number of concurrent changesets to process.
    pub changeset_concurrency: i64,
    /// Maximum size of blob sent in one query in bytes.
    pub max_blob_bytes: i64,
    /// Configuration for the content channel.
    pub content_channel_config: ModernSyncChannelConfig,
    /// Configuration for the filenodes channel.
    pub filenodes_channel_config: ModernSyncChannelConfig,
    /// Configuration for the trees channel.
    pub trees_channel_config: ModernSyncChannelConfig,
    /// Configuration for the changesets channel.
    pub changesets_channel_config: ModernSyncChannelConfig,
}

/// Configuration for a modern sync channel.
#[derive(Debug, PartialEq, Default, Clone, Eq)]
pub struct ModernSyncChannelConfig {
    /// Number of items to include in each batch.
    pub batch_size: i64,
    /// Total number of items to process in the channel.
    pub channel_size: i64,
    /// Interval between flushes in milliseconds.
    pub flush_interval_ms: i64,
}

/// Destination for telemetry logging.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LoggingDestination {
    /// Logs should be sent to the default logger for this type.
    Logger,
    /// Logs show be sent raw to scribe
    Scribe {
        /// Scribe category logs should be sent to
        scribe_category: String,
    },
}

/// Configuration for logging updates to the repo to external telemetry
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct UpdateLoggingConfig {
    /// Destination where bookmark updates are logged to
    pub bookmark_logging_destination: Option<LoggingDestination>,
    /// Destination where new commits are logged to
    pub new_commit_logging_destination: Option<LoggingDestination>,
    /// Destination where updates to git content refs are logged to
    pub git_content_refs_logging_destination: Option<LoggingDestination>,
}

/// Configuration for the commit graph
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct CommitGraphConfig {
    /// Scuba table to log commit graph operations to
    pub scuba_table: Option<String>,
    /// Blobstore key for a preloaded commit graph
    pub preloaded_commit_graph_blobstore_key: Option<String>,
    /// Whether to disable commit_graph_v2 queries that specify an empty common set for this repo.
    pub disable_commit_graph_v2_with_empty_common: bool,
}

/// Configuration for the repo metadata logger
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct MetadataLoggerConfig {
    /// Bookmarks to log repo metadata for
    pub bookmarks: Vec<BookmarkKey>,
    /// The interval time in secs for which the repo metadata logger sleeps between
    /// successive iterations of its incremental mode execution
    pub sleep_interval_secs: u64,
}

/// Configuration for connecting to Zelos
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum ZelosConfig {
    /// Connect to a local Zelos server
    Local {
        /// Local Zelos server port
        port: u16,
    },
    /// Connect to a remote Zelos server
    Remote {
        /// Remote Zelos server tier name
        tier: String,
    },
}

/// Configuration for git's bundle-uri feature
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct GitBundleURIConfig {
    /// Specifies what flavor of uri generator should be used.
    pub uri_generator_type: UriGeneratorType,
    /// Whether to support bundle-uri for untrusted clients
    pub trusted_only: bool,
}

/// Configuration for git's bundle-uri url-gen component
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum UriGeneratorType {
    /// Use CDN as the URI generator
    Cdn {
        /// Underlying storage bucket identifies
        bucket: String,
        /// Underlying storage bucket api-key
        api_key: String,
    },
    /// Use Manifold as the URI generator
    Manifold {
        /// Underlying storage bucket identifies
        bucket: String,
        /// Underlying storage bucket api-key
        api_key: String,
    },
    /// Use local FS as the URI generator
    LocalFS,
}

impl Default for UriGeneratorType {
    fn default() -> Self {
        Self::LocalFS
    }
}

/// Information on a loaded config
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct ConfigInfo {
    /// A hash of the raw config content
    pub content_hash: String,
    /// The time when the config was last updated
    pub last_updated_at: u64,
}

/// The concurrency setting to be used during git protocol
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct GitConcurrencyParams {
    /// The concurrency value for tree and blob fetches
    pub trees_and_blobs: usize,
    /// The concurrency value for commit fetches
    pub commits: usize,
    /// The concurrency value for tag fetches
    pub tags: usize,
    /// The concurrency value for boundary objects in shallow fetches
    pub shallow: usize,
}

/// All Git related configs (e.g. Git Server and Git-only repos)
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct GitConfigs {
    /// The concurrency setting to be used during git protocol for this repo
    pub git_concurrency: Option<GitConcurrencyParams>,
    /// Determines the behaviour on converting from Git commits
    /// to bonsais for this repo.
    ///  - With the flag ON the git lfs pointers will be interpreted and the actual file contents will
    ///    be stored. File contents have to be available in Mononoke.
    ///  - With this flag OFF the git lfs pointers are treated like any other file in the repo.
    pub git_lfs_interpret_pointers: bool,
    /// Optional messages to display to users after they run fetch commands (e.g.
    /// pull, clone).
    ///
    /// NOTE: Adding a message is not enough! The message will only be displayed
    /// if the repo enables this feature through a JK.
    pub fetch_message: Option<String>,
    /// Config for the git's bundle-uri feature
    pub git_bundle_uri: Option<GitBundleURIConfig>,
}

/// Configuration for x repo syncs
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct XRepoSyncSourceConfig {
    /// Regex matching the bookmarks that need to be synced
    pub bookmark_regex: String,
    /// Flag determining if backsyncing is enabled for this repo
    pub backsync_enabled: bool,
}

/// Configuration for x repo sync keyed by the target repo name
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct XRepoSyncSourceConfigMapping {
    /// Map of XRepoSyncSourceConfig for the current repo keyed by the name of the target repo, e.g.
    /// XRepoSyncSourceConfig for the sync from whatsapp/server to fbsource will be stored as
    /// whatsapp_server_config.mapping["fbsource"] = config
    pub mapping: BTreeMap<String, XRepoSyncSourceConfig>,
}

/// Configs that are being passed to commit cloud
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct CommitCloudConfig {
    /// Mock emails or usernames used for tests
    pub mocked_employees: Vec<String>,
    /// Disables interngraph notification whenever a commit is synced to commit cloud
    pub disable_interngraph_notification: bool,
}

/// Configs the multiplier when computing the repo load from the objects count
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ObjectsCountMultiplier(f32);

impl Eq for ObjectsCountMultiplier {}

impl ObjectsCountMultiplier {
    /// Build a new ObjectsCountMultiplier wrapping a value
    pub fn new(val: f32) -> Self {
        ObjectsCountMultiplier(val)
    }
}

impl Deref for ObjectsCountMultiplier {
    type Target = f32;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Enum controlling how certain Mononoke metadata cache is updated
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub enum MetadataCacheUpdateMode {
    /// Mode in which metadata cache updater tails the input category for updates
    Tailing {
        /// Scribe category to tail for updates
        category: String,
    },
    /// Mode in which metadata cache updater directly polls the XDB periodically for updates
    #[default]
    Polling,
}

/// Configuration for Mononoke metadata cache
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct MetadataCacheConfig {
    /// Mode for updating the warm bookmark metadata cache
    pub wbc_update_mode: Option<MetadataCacheUpdateMode>,
    /// Mode for updating the tags metadata cache
    pub tags_update_mode: Option<MetadataCacheUpdateMode>,
    /// Mode for updating the content refs metadata cache
    pub content_refs_update_mode: Option<MetadataCacheUpdateMode>,
}

impl MetadataCacheConfig {
    /// Returns the scribe category to tail for warm bookmark cache updates if it exists
    pub fn wbc_scribe_category(&self) -> Option<String> {
        match &self.wbc_update_mode {
            Some(MetadataCacheUpdateMode::Tailing { category }) => Some(category.to_string()),
            _ => None,
        }
    }

    /// Returns the scribe category to tail for tags cache updates if it exists
    pub fn tags_scribe_category(&self) -> Option<String> {
        match &self.tags_update_mode {
            Some(MetadataCacheUpdateMode::Tailing { category }) => Some(category.to_string()),
            _ => None,
        }
    }

    /// Returns the scribe category to tail for content refs cache updates if it exists
    pub fn content_refs_scribe_category(&self) -> Option<String> {
        match &self.content_refs_update_mode {
            Some(MetadataCacheUpdateMode::Tailing { category }) => Some(category.to_string()),
            _ => None,
        }
    }
}

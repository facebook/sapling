/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Contains structures describing configuration of the entire repo. Those structures are
//! deserialized from TOML files from metaconfig repo

#![deny(missing_docs)]
#![deny(warnings)]

use anyhow::{anyhow, Error, Result};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fmt, mem,
    num::{NonZeroU64, NonZeroUsize},
    ops::Deref,
    path::PathBuf,
    str,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use ascii::AsciiString;
use bookmarks_types::BookmarkName;
use mononoke_types::{MPath, RepositoryId};
use regex::Regex;
use scuba::ScubaValue;
use serde_derive::Deserialize;
use sql::mysql_async::{
    from_value_opt,
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};

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

/// Single entry that
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AllowlistEntry {
    /// Hardcoded allowed identity name i.e. USER (identity type) stash (identity data)
    HardcodedIdentity {
        /// Identity type
        ty: String,
        /// Identity data
        data: String,
    },
    /// Name of the tier
    Tier(String),
}

/// Configuration for all repos
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommonConfig {
    /// Who can interact with Mononoke
    pub security_config: Vec<AllowlistEntry>,
    /// Parent category to use for load limiting
    pub loadlimiter_category: Option<String>,
    /// Scuba table for logging redacted file accesses
    pub scuba_censored_table: Option<String>,
}

/// Configuration of a single repository
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct RepoConfig {
    /// If false, this repo config is completely ignored.
    pub enabled: bool,
    /// Persistent storage for this repo
    pub storage_config: StorageConfig,
    /// Address of the SQL database used to lock writes to a repo.
    pub write_lock_db_address: Option<String>,
    /// How large a cache to use (in bytes) for RepoGenCache derived information
    pub generation_cache_size: usize,
    /// Numerical repo id of the repo.
    pub repoid: RepositoryId,
    /// Scuba table for logging performance of operations
    pub scuba_table: Option<String>,
    /// Local file to log Scuba output to (useful in tests).
    pub scuba_local_path: Option<String>,
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
    /// Enables bookmarks cache with specified ttl (time to live)
    pub bookmarks_cache_ttl: Option<Duration>,
    /// Configuration for hooks
    pub hooks: Vec<HookParams>,
    /// Push configuration options
    pub push: PushParams,
    /// Pushrebase configuration options
    pub pushrebase: PushrebaseParams,
    /// LFS configuration options
    pub lfs: LfsParams,
    /// Configuration for logging all wireproto requests with full arguments.
    /// Used for replay on shadow tier.
    pub wireproto_logging: WireprotoLoggingConfig,
    /// What percent of read request verifies that returned content matches the hash
    pub hash_validation_percentage: usize,
    /// Should this repo reject write attempts
    pub readonly: RepoReadOnly,
    /// Should files be checked for redaction
    pub redaction: Redaction,
    /// Params for the hook manager
    pub hook_manager_params: Option<HookManagerParams>,
    /// Skiplist blobstore key (used to make revset faster)
    pub skiplist_index_blobstore_key: Option<String>,
    /// Params fro the bunle2 replay
    pub bundle2_replay_params: Bundle2ReplayParams,
    /// Max number of results in listkeyspatterns.
    pub list_keys_patterns_max: u64,
    /// Params for File storage
    pub filestore: Option<FilestoreParams>,
    /// Config for commit sync
    pub commit_sync_config: Option<CommitSyncConfig>,
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
    /// Name of this repository in hgsql.
    pub hgsql_name: HgsqlName,
    /// Name of this repository in hgsql ... for globalrevs. This could, in some cases, not be the
    /// same as HgsqlName.
    pub hgsql_globalrevs_name: HgsqlGlobalrevsName,
    /// Whether to enforce strict LFS ACL checks for this repo.
    pub enforce_lfs_acl_check: bool,
    /// Whether to use warm bookmark cache while serving data hg wireprotocol
    pub repo_client_use_warm_bookmarks_cache: bool,
    /// Configuration for Segmented Changelog.
    pub segmented_changelog_config: SegmentedChangelogConfig,
    /// Do not consider bookmark warm unless blobimport processed it.
    /// That means that changeset is present in both Mononoke and hg.
    pub warm_bookmark_cache_check_blobimport: bool,
    /// Configuration for repo_client module
    pub repo_client_knobs: RepoClientKnobs,
    /// Callsign to check phabricator commits
    pub phabricator_callsign: Option<String>,
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
    /// Types of derived data that can be derived for this repo
    pub derived_data_types: BTreeSet<String>,
    /// What unode version should be used (defaults to V1)
    pub unode_version: UnodeVersion,
    /// Override the file size limit for blame. Blame won't be derived for files which
    /// size is above the limit. NOTE: if `override_blame_filesize_limit` is None
    /// then a default limit will be used!
    pub override_blame_filesize_limit: Option<u64>,
}

/// What type of unode derived data to generate
#[derive(Eq, Clone, Copy, Debug, PartialEq)]
pub enum UnodeVersion {
    /// Unodes v1
    V1,
    /// Unodes v2
    V2,
}

impl Default for UnodeVersion {
    fn default() -> Self {
        UnodeVersion::V1
    }
}

impl RepoConfig {
    /// Returns the address of the primary metadata database, or None if there is none.
    pub fn primary_metadata_db_address(&self) -> Option<String> {
        self.storage_config.metadata.primary_address()
    }
}

#[derive(Eq, Copy, Clone, Debug, PartialEq, Deserialize)]
/// Should the redaction verification be enabled?
pub enum Redaction {
    /// Redacted files cannot be accessed
    Enabled,
    /// All the files can be fetched
    Disabled,
}

impl Default for Redaction {
    fn default() -> Self {
        Redaction::Enabled
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
/// Is the repo read-only?
pub enum RepoReadOnly {
    /// This repo is read-only and should not accept pushes or other writes
    ReadOnly(String),
    /// This repo should accept writes.
    ReadWrite,
}

impl Default for RepoReadOnly {
    fn default() -> Self {
        RepoReadOnly::ReadWrite
    }
}

/// Configuration of warming up the Mononoke cache. This warmup happens on startup
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CacheWarmupParams {
    /// Bookmark to warmup cache for at the startup. If not set then the cache will be cold.
    pub bookmark: BookmarkName,
    /// Max number to fetch during commit warmup. If not set in the config, then set to a default
    /// value.
    pub commit_limit: usize,
    /// Whether to use microwave to accelerate cache warmup.
    pub microwave_preload: bool,
}

/// Configuration for the hook manager
#[derive(Debug, Clone, Eq, PartialEq, Deserialize)]
pub struct HookManagerParams {
    /// Wether to disable the acl checker or not (intended for testing purposes)
    pub disable_acl_checker: bool,
}

impl Default for HookManagerParams {
    fn default() -> Self {
        Self {
            disable_acl_checker: false,
        }
    }
}

/// Configuration might be done for a single bookmark or for all bookmarks matching a regex
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BookmarkOrRegex {
    /// Matches a single bookmark
    Bookmark(BookmarkName),
    /// Matches bookmarks with a regex
    Regex(ComparableRegex),
}

impl BookmarkOrRegex {
    /// Checks whether a given Bookmark matches this bookmark or regex
    pub fn matches(&self, bookmark: &BookmarkName) -> bool {
        match self {
            BookmarkOrRegex::Bookmark(ref bm) => bm.eq(bookmark),
            BookmarkOrRegex::Regex(ref re) => re.is_match(&bookmark.to_string()),
        }
    }
}

impl From<BookmarkName> for BookmarkOrRegex {
    fn from(b: BookmarkName) -> Self {
        BookmarkOrRegex::Bookmark(b)
    }
}

impl From<Regex> for BookmarkOrRegex {
    fn from(r: Regex) -> Self {
        BookmarkOrRegex::Regex(ComparableRegex(r))
    }
}

/// Collection of all bookmark attribtes
#[derive(Clone)]
pub struct BookmarkAttrs {
    bookmark_params: Arc<Vec<BookmarkParams>>,
}

impl BookmarkAttrs {
    /// create bookmark attributes from bookmark params vector
    pub fn new(bookmark_params: impl Into<Arc<Vec<BookmarkParams>>>) -> Self {
        Self {
            bookmark_params: bookmark_params.into(),
        }
    }

    /// select bookmark params matching provided bookmark
    pub fn select<'a>(
        &'a self,
        bookmark: &'a BookmarkName,
    ) -> impl Iterator<Item = &'a BookmarkParams> {
        self.bookmark_params
            .iter()
            .filter(move |params| params.bookmark.matches(bookmark))
    }

    /// check if provided bookmark is fast-forward only
    pub fn is_fast_forward_only(&self, bookmark: &BookmarkName) -> bool {
        self.select(bookmark).any(|params| params.only_fast_forward)
    }

    /// Check if a bookmark config overrides whether date should be rewritten during pushrebase.
    /// Return None if there are no bookmark config overriding rewrite_dates.
    pub fn should_rewrite_dates(&self, bookmark: &BookmarkName) -> Option<bool> {
        for params in self.select(bookmark) {
            // NOTE: If there are multiple patterns matching the bookmark, the first match
            // overrides others. It might not be the most desired behavior, though.
            if let Some(rewrite_dates) = params.rewrite_dates {
                return Some(rewrite_dates);
            }
        }
        None
    }

    /// check if provided unix name is allowed to move specified bookmark
    pub fn is_allowed_user(&self, user: &Option<String>, bookmark: &BookmarkName) -> bool {
        match user {
            None => true,
            Some(user) => {
                // NOTE: `Iterator::all` combinator returns `true` if selected set is empty
                //       which is consistent with what we want
                self.select(bookmark)
                    .flat_map(|params| &params.allowed_users)
                    .all(|re| re.is_match(user))
            }
        }
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
    /// Only users matching this pattern will be allowed to move this bookmark
    pub allowed_users: Option<ComparableRegex>,
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
pub enum HookBypass {
    /// Bypass that checks that a string is in the commit message
    CommitMessage(String),
    /// Bypass that checks that a string is in the commit message
    Pushvar {
        /// Name of the pushvar
        name: String,
        /// Value of the pushvar
        value: String,
    },
}

/// Configs that are being passed to the hook during runtime
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct HookConfig {
    /// An optional way to bypass a hook
    pub bypass: Option<HookBypass>,
    /// Map of config to it's value. Values here are strings
    pub strings: HashMap<String, String>,
    /// Map of config to it's value. Values here are integers
    pub ints: HashMap<String, i32>,
}

/// Configuration for a hook
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookParams {
    /// The name of the hook
    pub name: String,
    /// Configs that should be passed to hook
    pub config: HookConfig,
}

/// Push configuration options
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PushParams {
    /// Whether normal non-pushrebase pushes are allowed
    pub pure_push_allowed: bool,
    /// Scribe category we log new commits to
    pub commit_scribe_category: Option<String>,
}

impl Default for PushParams {
    fn default() -> Self {
        PushParams {
            pure_push_allowed: true,
            commit_scribe_category: None,
        }
    }
}

/// Flags for the pushrebase inner loop
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct PushrebaseFlags {
    /// Update dates of rebased commits
    pub rewritedates: bool,
    /// How far will we go from bookmark to find rebase root
    pub recursion_limit: Option<usize>,
    /// Forbid rebases when root is not a p1 of the rebase set.
    pub forbid_p2_root_rebases: bool,
    /// Whether to do chasefolding check during pushrebase
    pub casefolding_check: bool,
    /// How many commits are allowed to not have filenodes generated.
    pub not_generated_filenodes_limit: u64,
}

impl Default for PushrebaseFlags {
    fn default() -> Self {
        PushrebaseFlags {
            rewritedates: true,
            recursion_limit: Some(16384), // this number is fairly arbirary
            forbid_p2_root_rebases: true,
            casefolding_check: true,
            not_generated_filenodes_limit: 500,
        }
    }
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
    /// Scribe category we log new commits to
    pub commit_scribe_category: Option<String>,
    /// Whether Globalrevs should be assigned
    pub assign_globalrevs: bool,
    /// Whether Git Mapping should be populated from extras (affects also blobimport)
    pub populate_git_mapping: bool,
}

impl Default for PushrebaseParams {
    fn default() -> Self {
        PushrebaseParams {
            flags: PushrebaseFlags::default(),
            block_merges: false,
            emit_obsmarkers: false,
            commit_scribe_category: None,
            assign_globalrevs: false,
            populate_git_mapping: false,
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
    /// Whether hg sync job should generate lfs blobs
    pub generate_lfs_blob_in_hg_sync_job: bool,
    /// Hosts in this smc tier will receive lfs pointers regardless of rollout_percentage
    pub rollout_smc_tier: Option<String>,
}

/// Id used to discriminate diffirent underlying blobstore instances
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Deserialize)]
pub struct BlobstoreId(u64);

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

impl From<BlobstoreId> for Value {
    fn from(id: BlobstoreId) -> Self {
        Value::UInt(id.0)
    }
}

impl ConvIr<BlobstoreId> for BlobstoreId {
    fn new(v: Value) -> Result<Self, FromValueError> {
        Ok(BlobstoreId(from_value_opt(v)?))
    }
    fn commit(self) -> Self {
        self
    }
    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for BlobstoreId {
    type Intermediate = BlobstoreId;
}

impl From<BlobstoreId> for ScubaValue {
    fn from(blobstore_id: BlobstoreId) -> Self {
        ScubaValue::from(blobstore_id.0 as i64)
    }
}

/// Id used to identify storage configuration for a multiplexed blobstore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct MultiplexId(i32);

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

impl From<MultiplexId> for Value {
    fn from(id: MultiplexId) -> Self {
        Value::Int(id.0.into())
    }
}

impl ConvIr<MultiplexId> for MultiplexId {
    fn new(v: Value) -> Result<Self, FromValueError> {
        Ok(MultiplexId(from_value_opt(v)?))
    }
    fn commit(self) -> Self {
        self
    }
    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for MultiplexId {
    type Intermediate = MultiplexId;
}

/// Define storage needed for repo.
/// Storage consists of a blobstore and some kind of SQL DB for metadata. The configurations
/// can be broadly classified as "local" and "remote". "Local" is primarily for testing, and is
/// only suitable for single hosts. "Remote" is durable storage which can be shared by multiple
/// BlobRepo instances on different hosts.
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct StorageConfig {
    /// Blobstores. If the blobstore has a BlobstoreId then it can be used as a component of
    /// a Multiplexed blobstore.
    pub blobstore: BlobConfig,
    /// Metadata database
    pub metadata: MetadataDatabaseConfig,
}

/// What to do when the ScrubBlobstore finds a problem
#[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize)]
pub enum ScrubAction {
    /// Log items needing repair
    ReportOnly,
    /// Do repairs
    Repair,
}

impl FromStr for ScrubAction {
    type Err = Error;

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        match string {
            "ReportOnly" => Ok(ScrubAction::ReportOnly),
            "Repair" => Ok(ScrubAction::Repair),
            _ => Err(anyhow!("Unable to parse {} as {}", string, "ScrubAction")),
        }
    }
}

/// Whether we should read from this blobstore normally in a Multiplex,
/// or only read from it in Scrub or when it's our last chance to find the blob
#[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize)]
pub enum MultiplexedStoreType {
    /// Normal operation, no special treatment
    Normal,
    /// Only read if Normal blobstores don't provide the blob. Writes go here as per normal
    WriteMostly,
}

/// Configuration for a blobstore
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BlobConfig {
    /// Administratively disabled blobstore
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
    /// Multiplex across multiple blobstores for redundancy
    Multiplexed {
        /// A unique ID that identifies this multiplex configuration
        multiplex_id: MultiplexId,
        /// A scuba table I guess
        scuba_table: Option<String>,
        /// Set of blobstores being multiplexed over
        blobstores: Vec<(BlobstoreId, MultiplexedStoreType, BlobConfig)>,
        /// The number of writes that must succeed for a `put` to the multiplex to succeed
        minimum_successful_writes: NonZeroUsize,
        /// 1 in scuba_sample_rate samples will be logged.
        scuba_sample_rate: NonZeroU64,
        /// DB config to use for the sync queue
        queue_db: DatabaseConfig,
    },
    /// Multiplex across multiple blobstores scrubbing for errors
    Scrub {
        /// A unique ID that identifies this multiplex configuration
        multiplex_id: MultiplexId,
        /// A scuba table I guess
        scuba_table: Option<String>,
        /// Set of blobstores being multiplexed over
        blobstores: Vec<(BlobstoreId, MultiplexedStoreType, BlobConfig)>,
        /// The number of writes that must succeed for a `put` to the multiplex to succeed
        minimum_successful_writes: NonZeroUsize,
        /// Whether to attempt repair
        scrub_action: ScrubAction,
        /// 1 in scuba_sample_rate samples will be logged.
        scuba_sample_rate: NonZeroU64,
        /// DB config to use for the sync queue
        queue_db: DatabaseConfig,
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
    },
}

impl BlobConfig {
    /// Return true if the blobstore is strictly local. Multiplexed blobstores are local iff
    /// all their components are.
    pub fn is_local(&self) -> bool {
        use BlobConfig::*;

        match self {
            Disabled | Files { .. } | Sqlite { .. } => true,
            Manifold { .. } | Mysql { .. } | ManifoldWithTtl { .. } => false,
            Multiplexed { blobstores, .. } | Scrub { blobstores, .. } => blobstores
                .iter()
                .map(|(_, _, config)| config)
                .all(BlobConfig::is_local),
            Logging { blobconfig, .. } => blobconfig.is_local(),
            Pack { blobconfig, .. } => blobconfig.is_local(),
        }
    }

    /// Change all internal blobstores to scrub themselves for errors where possible.
    /// This maximises error rates, and asks blobstores to silently fix errors when they are able
    /// to do so - ideal for repository checkers.
    pub fn set_scrubbed(&mut self, scrub_action: ScrubAction) {
        use BlobConfig::{Multiplexed, Scrub};

        if let Multiplexed {
            multiplex_id,
            scuba_table,
            scuba_sample_rate,
            blobstores,
            minimum_successful_writes,
            queue_db,
        } = self
        {
            let scuba_table = mem::replace(scuba_table, None);
            let mut blobstores = mem::replace(blobstores, Vec::new());
            for (_, _, store) in blobstores.iter_mut() {
                store.set_scrubbed(scrub_action);
            }
            *self = Scrub {
                multiplex_id: *multiplex_id,
                scuba_table,
                scuba_sample_rate: *scuba_sample_rate,
                blobstores,
                minimum_successful_writes: *minimum_successful_writes,
                scrub_action,
                queue_db: queue_db.clone(),
            };
        }
    }
}

impl Default for BlobConfig {
    fn default() -> Self {
        BlobConfig::Disabled
    }
}

/// Configuration for a local SQLite database
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LocalDatabaseConfig {
    /// Path to the directory containing the SQLite databases
    pub path: PathBuf,
}

/// Configuration for a remote MySQL database
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RemoteDatabaseConfig {
    /// SQL database to connect to
    pub db_address: String,
}

/// Configuration for a sharded remote MySQL database
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ShardedRemoteDatabaseConfig {
    /// SQL database shard map to connect to
    pub shard_map: String,
    /// Number of shards to distribute data across.
    pub shard_num: NonZeroUsize,
}

/// Configuration for a potentially sharded remote MySQL database
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ShardableRemoteDatabaseConfig {
    /// Database is not sharded.
    Unsharded(RemoteDatabaseConfig),
    /// Database is sharded.
    Sharded(ShardedRemoteDatabaseConfig),
}

/// Configuration for a single database
#[derive(Debug, Clone, Eq, PartialEq)]
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

/// Configuration for the Metadata database when it is remote.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RemoteMetadataDatabaseConfig {
    /// Database for the primary metadata.
    pub primary: RemoteDatabaseConfig,
    /// Database for possibly sharded filenodes.
    pub filenodes: ShardableRemoteDatabaseConfig,
    /// Database for commit mutation metadata.
    pub mutation: RemoteDatabaseConfig,
}

/// Configuration for the Metadata database
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum MetadataDatabaseConfig {
    /// Local SQLite database
    Local(LocalDatabaseConfig),
    /// Remote MySQL databases
    Remote(RemoteMetadataDatabaseConfig),
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
        }
    }

    /// The address of the primary metadata database, if this is a remote metadata database.
    pub fn primary_address(&self) -> Option<String> {
        match self {
            MetadataDatabaseConfig::Remote(remote) => Some(remote.primary.db_address.clone()),
            MetadataDatabaseConfig::Local(_) => None,
        }
    }
}

/// Params for the bundle2 replay
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub struct Bundle2ReplayParams {
    /// A flag specifying whether to preserve raw bundle2 contents in the blobstore
    pub preserve_raw_bundle2: bool,
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
    pub fn matches_bookmark(&self, bookmark: &BookmarkName) -> bool {
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
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InfinitepushParams {
    /// Whether infinite push bundles are allowed on this server. If false, all infinitepush
    /// bundles will be rejected.
    pub allow_writes: bool,

    /// Valid namespace for infinite push bookmarks. If None, then infinitepush bookmarks are not
    /// allowed.
    pub namespace: Option<InfinitepushNamespace>,

    /// Whether to put trees/files in the getbundle response for infinitepush commits
    pub hydrate_getbundle_response: bool,

    /// Whether to write saved infinitepush bundles into the reverse filler queue
    pub populate_reverse_filler_queue: bool,

    /// Scribe category we log new commits to
    pub commit_scribe_category: Option<String>,
}

impl Default for InfinitepushParams {
    fn default() -> Self {
        Self {
            allow_writes: false,
            namespace: None,
            hydrate_getbundle_response: false,
            populate_reverse_filler_queue: false,
            commit_scribe_category: None,
        }
    }
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
    PrependPrefix(MPath),
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
    pub map: HashMap<MPath, MPath>,
    /// Bookmark prefix to use in the large repo
    pub bookmark_prefix: AsciiString,
    /// Commit sync direction
    pub direction: CommitSyncDirection,
}

/// Commit sync direction
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CommitSyncDirection {
    /// Syncing commits from large repo to small ones
    LargeToSmall,
    /// Syncing commits from small repos to large one
    SmallToLarge,
}

/// CommitSyncConfig version name
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CommitSyncConfigVersion(pub String);

impl fmt::Display for CommitSyncConfigVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Commit sync configuration for a large repo
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommitSyncConfig {
    /// Large repository id
    pub large_repo_id: RepositoryId,
    /// Common pushrebase bookmarks
    pub common_pushrebase_bookmarks: Vec<BookmarkName>,
    /// Corresponding small repo configs
    pub small_repos: HashMap<RepositoryId, SmallRepoCommitSyncConfig>,
    /// Version name of the commit sync config
    pub version_name: CommitSyncConfigVersion,
}

/// Configuration for logging wireproto commands and arguments
/// This is used by traffic replay script to replay on prod traffic on shadow tier
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct WireprotoLoggingConfig {
    /// Scribe category to log to
    pub scribe_category: Option<String>,
    /// Storage config to store wireproto arguments. The arguments can be quite big,
    /// so storing separately would make sense.
    /// Second parameter is threshold. If wireproto arguments are bigger than this threshold
    /// then they will be stored in remote storage defined by first parameter. Note that if
    /// `storage_config_and_threshold` is not specified then wireproto wireproto arguments will
    /// be inlined
    pub storage_config_and_threshold: Option<(StorageConfig, u64)>,
    /// Local path where to log replay data that would be sent to Scribe.
    pub local_path: Option<String>,
}

impl Default for WireprotoLoggingConfig {
    fn default() -> Self {
        Self {
            scribe_category: None,
            storage_config_and_threshold: None,
            local_path: None,
        }
    }
}

/// Source Control Service options
#[derive(Debug, Clone, Eq, PartialEq)]
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
}

impl Default for SourceControlServiceParams {
    fn default() -> Self {
        SourceControlServiceParams {
            permit_writes: false,
            permit_service_writes: false,
            service_write_hipster_acl: None,
            service_write_restrictions: HashMap::new(),
        }
    }
}

/// Restrictions on writes for services.
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct ServiceWriteRestrictions {
    /// The service is permissed to call these methods
    pub permitted_methods: HashSet<String>,

    /// The service is permitted to modify files with these path prefixes.
    pub permitted_path_prefixes: BTreeSet<Option<MPath>>,

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
    pub bookmarks_to_report_age: Vec<BookmarkName>,
}

/// Represents the repository name for this repository in Hgsql.
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct HgsqlName(pub String);

impl AsRef<str> for HgsqlName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<String> for HgsqlName {
    fn as_ref(&self) -> &String {
        &self.0
    }
}

/// Represents the repository name for Globalrevs for this repository in Hgsql.
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct HgsqlGlobalrevsName(pub String);

impl AsRef<str> for HgsqlGlobalrevsName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<String> for HgsqlGlobalrevsName {
    fn as_ref(&self) -> &String {
        &self.0
    }
}

/// Configuration for Segmented Changelog.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SegmentedChangelogConfig {
    /// Signals whether segmented changelog functionality is enabled for the current repository.
    /// This can mean that functionality is disabled to shed load, that the required data is not
    /// curretly being computed or that it was never computed for this repository.
    pub enabled: bool,
}

impl Default for SegmentedChangelogConfig {
    fn default() -> Self {
        SegmentedChangelogConfig { enabled: false }
    }
}

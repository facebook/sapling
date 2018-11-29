// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Contains structures describing configuration of the entire repo. Those structures are
//! deserialized from TOML files from metaconfig repo

use blobrepo::{BlobRepo, ManifoldArgs};
use bookmarks::Bookmark;
use bytes::Bytes;
use errors::*;
use failure::chain::ChainExt;
use futures::{failed, finished, future, Future};
use futures::Stream;
use futures_ext::FutureExt;
use mercurial_types::{Changeset, MPath, MPathElement, Manifest};
use mercurial_types::manifest::Content;
use mercurial_types::nodehash::HgChangesetId;
use mononoke_types::FileContents;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str;
use toml;
use vfs::{vfs_from_manifest, ManifestVfsDir, ManifestVfsFile, VfsDir, VfsFile, VfsNode, VfsWalker};

/// Configuration of a single repository
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RepoConfig {
    /// If false, this repo config is completely ignored.
    pub enabled: bool,
    /// Defines the type of repository
    pub repotype: RepoType,
    /// How large a cache to use (in bytes) for RepoGenCache derived information
    pub generation_cache_size: usize,
    /// Numerical repo id of the repo.
    pub repoid: i32,
    /// Scuba table for logging performance of operations
    pub scuba_table: Option<String>,
    /// Parameters of how to warm up the cache
    pub cache_warmup: Option<CacheWarmupParams>,
    /// Configuration for bookmarks
    pub bookmarks: Option<Vec<BookmarkParams>>,
    /// Configuration for hooks
    pub hooks: Option<Vec<HookParams>>,
    /// Pushrebase configuration options
    pub pushrebase: PushrebaseParams,
    /// LFS configuration options
    pub lfs: LfsParams,
    /// Scribe category to log all wireproto requests with full arguments.
    /// Used for replay on shadow tier.
    pub wireproto_scribe_category: Option<String>,
    /// What percent of read request verifies that returned content matches the hash
    pub hash_validation_percentage: usize,
    /// Should this repo reject write attempts
    pub readonly: RepoReadOnly,
    /// Params for the hook manager
    pub hook_manager_params: Option<HookManagerParams>,
    /// Skiplist blobstore key (used to make revset faster)
    pub skiplist_index_blobstore_key: Option<String>,
}

impl RepoConfig {
    /// Returns a db address that is referenced in this config or None if there is none
    pub fn get_db_address(&self) -> Option<&str> {
        match self.repotype {
            RepoType::BlobManifold(ref args) => Some(&args.db_address),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
/// Is the repo read-only?
pub enum RepoReadOnly {
    /// This repo is read-only and should not accept pushes or other writes
    ReadOnly,
    /// This repo should accept writes.
    ReadWrite,
}

/// Configuration of warming up the Mononoke cache. This warmup happens on startup
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CacheWarmupParams {
    /// Bookmark to warmup cache for at the startup. If not set then the cache will be cold.
    pub bookmark: Bookmark,
    /// Max number to fetch during commit warmup. If not set in the config, then set to a default
    /// value.
    pub commit_limit: usize,
}

/// Configuration for the hook manager
#[derive(Debug, Clone, Eq, PartialEq, Deserialize)]
pub struct HookManagerParams {
    /// Entry limit for the hook manager result cache
    pub entrylimit: usize,

    /// Weight limit for the hook manager result cache
    pub weightlimit: usize,
}

impl Default for HookManagerParams {
    fn default() -> Self {
        Self {
            entrylimit: 1024 * 1024,
            weightlimit: 100 * 1024 * 1024, // 100Mb
        }
    }
}

/// Configuration for a bookmark
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BookmarkParams {
    /// The bookmark
    pub bookmark: Bookmark,
    /// The hooks active for the bookmark
    pub hooks: Option<Vec<String>>,
}

/// The type of the hook
#[derive(Debug, Clone, Eq, PartialEq, Deserialize)]
pub enum HookType {
    /// A hook that runs on the whole changeset
    PerChangeset,
    /// A hook that runs on a file in a changeset
    PerAddedOrModifiedFile,
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

/// Configuration for a hook
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookParams {
    /// The name of the hook
    pub name: String,
    /// The type of the hook
    pub hook_type: HookType,
    /// The code of the hook
    pub code: Option<String>,
    /// An optional way to bypass a hook
    pub bypass: Option<HookBypass>,
}

/// Pushrebase configuration options
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PushrebaseParams {
    /// Update dates of rebased commits
    pub rewritedates: bool,
    /// How far will we go from bookmark to find rebase root
    pub recursion_limit: usize,
}

impl Default for PushrebaseParams {
    fn default() -> Self {
        PushrebaseParams {
            rewritedates: true,
            recursion_limit: 16384, // this number is fairly arbirary
        }
    }
}

/// LFS configuration options
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LfsParams {
    /// threshold in bytes, If None, Lfs is disabled
    pub threshold: Option<u64>,
}

impl Default for LfsParams {
    fn default() -> Self {
        LfsParams { threshold: None }
    }
}

/// Types of repositories supported
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RepoType {
    /// Blob repository with path pointing to on-disk files with data. The files are stored in a
    ///
    ///
    /// NOTE: this is read-only and for development/testing only. Production uses will break things.
    BlobFiles(PathBuf),
    /// Blob repository with path pointing to on-disk files with data. The files are stored in a
    /// RocksDb database
    BlobRocks(PathBuf),
    /// Blob repository with path pointing to the directory where a server socket is going to be.
    BlobManifold(ManifoldArgs),
    /// Blob repository with path pointing to on-disk files with data. The files are stored in a
    /// RocksDb database, and a log-normal delay is applied to access to simulate a remote store
    /// like Manifold. Params are path, mean microseconds, stddev microseconds.
    TestBlobDelayRocks(PathBuf, u64, u64),
}

/// Configuration of a metaconfig repository
#[derive(Debug, Eq, PartialEq)]
pub struct MetaConfig {}

/// Holds configuration all configuration that was read from metaconfig repository's manifest.
#[derive(Debug, PartialEq)]
pub struct RepoConfigs {
    /// Config for the config repository
    pub metaconfig: MetaConfig,
    /// Configs for all other repositories
    pub repos: HashMap<String, RepoConfig>,
}

impl RepoConfigs {
    /// Read the config repo and generate RepoConfigs based on it
    pub fn read_config_repo(
        repo: BlobRepo,
        changesetid: HgChangesetId,
    ) -> Box<Future<Item = Self, Error = Error> + Send> {
        Box::new(
            repo.get_changeset_by_changesetid(&changesetid)
                .and_then(move |changeset| {
                    repo.get_manifest_by_nodeid(&changeset.manifestid().clone())
                })
                .chain_err("failed to get manifest from changeset")
                .from_err()
                .and_then(|manifest| Self::read_manifest(&manifest))
                .chain_err("While reading config repo")
                .from_err(),
        )
    }

    /// Read the given manifest of metaconfig repo and yield the RepoConfigs for it
    fn read_manifest<M>(manifest: &M) -> Box<Future<Item = Self, Error = Error> + Send>
    where
        M: Manifest,
    {
        Box::new(
            vfs_from_manifest(manifest)
                .from_err()
                .and_then(|vfs| RepoConfigs::read_repos(vfs.into_node())),
        )
    }

    fn read_repos(
        root_node: VfsNode<ManifestVfsDir, ManifestVfsFile>,
    ) -> Box<Future<Item = Self, Error = Error> + Send> {
        Box::new(
            finished(root_node.clone())
                .and_then(|root_node| {
                    let path = try_boxfuture!(MPath::new(b"repos"));
                    VfsWalker::new(root_node, path).walk()
                })
                .and_then(|repos_node| match repos_node {
                    VfsNode::File(_) => {
                        bail_err!(ErrorKind::InvalidFileStructure("expected directory".into()))
                    }
                    VfsNode::Dir(dir) => Ok(dir),
                })
                .and_then(move |repos_dir| {
                    let repodirs: Vec<_> = repos_dir.read().into_iter().cloned().collect();
                    let repos_node = repos_dir.into_node();
                    future::join_all(repodirs.into_iter().map(move |repodir| {
                        Self::read_repo(root_node.clone(), repos_node.clone(), repodir)
                    }))
                })
                .map(|repos| RepoConfigs {
                    metaconfig: MetaConfig {},
                    repos: repos.into_iter().collect(),
                }),
        )
    }

    fn read_repo(
        root_node: VfsNode<ManifestVfsDir, ManifestVfsFile>,
        repos_dir: VfsNode<ManifestVfsDir, ManifestVfsFile>,
        repo_dir: MPathElement,
    ) -> Box<Future<Item = (String, RepoConfig), Error = Error> + Send> {
        let repo_name = try_boxfuture!(str::from_utf8(repo_dir.as_bytes())).to_string();

        VfsWalker::new(repos_dir, repo_dir.into_iter().cloned())
            .walk()
            .from_err()
            .and_then(|node| match node {
                VfsNode::Dir(dir) => Ok(dir),
                _ => Err(ErrorKind::InvalidFileStructure("expected directory".into()).into()),
            })
            .and_then(|repo_dir| {
                RepoConfigs::read_file(
                    repo_dir.clone().into_node(),
                    try_boxfuture!(MPath::new(b"server.toml".to_vec())),
                ).map(move |bytes| (bytes, repo_dir))
                    .boxify()
            })
            .and_then(|(bytes, repo_dir)| {
                let raw_config = try_boxfuture!(toml::from_slice::<RawRepoConfig>(bytes.as_ref()));
                let hooks = raw_config.hooks.clone();
                // Easier to deal with empty vector than Option
                let hooks = hooks.unwrap_or(Vec::new());
                future::join_all(hooks.into_iter().map(move |raw_hook_config| {
                    let bypass = RepoConfigs::get_bypass(raw_hook_config.clone());
                    if raw_hook_config.name.starts_with("rust:") {
                        // No need to load lua code for rust hook
                        match bypass {
                            Ok(bypass) => finished(HookParams {
                                name: raw_hook_config.name,
                                code: None,
                                hook_type: raw_hook_config.hook_type,
                                bypass,
                            }).boxify(),
                            Err(e) => failed(e).boxify(),
                        }
                    } else {
                        let path = raw_hook_config.path.clone();
                        let path = match path {
                            Some(path) => path,
                            None => return failed(ErrorKind::MissingPath().into()).boxify(),
                        };
                        let relative_prefix = "./";
                        let is_relative = path.starts_with(relative_prefix);
                        let path_node;
                        let path_adjusted;
                        if is_relative {
                            path_node = repo_dir.clone().into_node();
                            path_adjusted = path.chars().skip(relative_prefix.len()).collect();
                        } else {
                            path_node = root_node.clone();
                            path_adjusted = path;
                        }
                        RepoConfigs::read_file(
                            path_node,
                            try_boxfuture!(MPath::new(path_adjusted.as_bytes().to_vec())),
                        ).and_then(|bytes| {
                            let code = str::from_utf8(&bytes)?;
                            let code = code.to_string();
                            match bypass {
                                Ok(bypass) => Ok(HookParams {
                                    name: raw_hook_config.name,
                                    code: Some(code),
                                    hook_type: raw_hook_config.hook_type,
                                    bypass,
                                }),
                                Err(e) => Err(e),
                            }
                        })
                            .boxify()
                    }
                })).map(|hook_params| (raw_config, hook_params))
                    .boxify()
            })
            .then(|res| match res {
                Ok((raw_config, all_hook_params)) => Ok((
                    repo_name,
                    RepoConfigs::convert_conf(raw_config, all_hook_params)?,
                )),
                Err(e) => Err(e),
            })
            .boxify()
    }

    fn get_bypass(raw_hook_config: RawHookConfig) -> Result<Option<HookBypass>> {
        let bypass_commit_message = raw_hook_config
            .bypass_commit_string
            .map(|s| HookBypass::CommitMessage(s));

        let bypass_pushvar = raw_hook_config.bypass_pushvar.and_then(|s| {
            let pushvar: Vec<_> = s.split('=').map(|val| val.to_string()).collect();
            if pushvar.len() != 2 {
                return Some(Err(ErrorKind::InvalidPushvar(s).into()));
            }
            Some(Ok((
                pushvar.get(0).unwrap().clone(),
                pushvar.get(1).unwrap().clone(),
            )))
        });
        let bypass_pushvar = match bypass_pushvar {
            Some(Err(err)) => {
                return Err(err);
            }
            Some(Ok((name, value))) => Some(HookBypass::Pushvar { name, value }),
            None => None,
        };

        if bypass_commit_message.is_some() && bypass_pushvar.is_some() {
            return Err(ErrorKind::TooManyBypassOptions(raw_hook_config.name).into());
        }
        let bypass = bypass_commit_message.or(bypass_pushvar);

        Ok(bypass)
    }

    fn read_file(
        file_dir: VfsNode<ManifestVfsDir, ManifestVfsFile>,
        file_path: MPath,
    ) -> impl Future<Item = Bytes, Error = Error> {
        VfsWalker::new(file_dir, file_path.clone())
            .collect()
            .and_then(move |nodes| {
                nodes
                    .last()
                    .cloned()
                    .ok_or(ErrorKind::InvalidPath(file_path).into())
            })
            .and_then(|node| match node {
                VfsNode::File(file) => Ok(file),
                _ => Err(ErrorKind::InvalidFileStructure("expected file".into()).into()),
            })
            .and_then(|file| {
                file.read()
                    .map_err(|err| err.context("failed to read content of the file").into())
            })
            .and_then(|content| match content {
                Content::File(FileContents::Bytes(bytes)) => Ok(bytes),
                _ => Err(ErrorKind::InvalidFileStructure("expected file".into()).into()),
            })
    }

    fn convert_conf(this: RawRepoConfig, hooks: Vec<HookParams>) -> Result<RepoConfig> {
        fn get_path(config: &RawRepoConfig) -> ::std::result::Result<PathBuf, ErrorKind> {
            config.path.clone().ok_or_else(|| {
                ErrorKind::InvalidConfig(format!(
                    "No path provided for {:#?} type of repo",
                    config.repotype
                ))
            })
        }

        let repotype = match this.repotype {
            RawRepoType::Files => RepoType::BlobFiles(get_path(&this)?),
            RawRepoType::BlobRocks => RepoType::BlobRocks(get_path(&this)?),
            RawRepoType::TestBlobManifold => {
                let manifold_bucket = this.manifold_bucket.ok_or(ErrorKind::InvalidConfig(
                    "manifold bucket must be specified".into(),
                ))?;
                let db_address = this.db_address.expect("xdb tier was not specified");
                RepoType::BlobManifold(ManifoldArgs {
                    bucket: manifold_bucket,
                    prefix: this.manifold_prefix.unwrap_or("".into()),
                    db_address,
                    filenode_shards: this.filenode_shards,
                })
            }
            RawRepoType::TestBlobDelayRocks => RepoType::TestBlobDelayRocks(
                get_path(&this)?,
                this.delay_mean.expect("mean delay must be specified"),
                this.delay_stddev.expect("stddev delay must be specified"),
            ),
        };

        let enabled = this.enabled.unwrap_or(true);
        let generation_cache_size = this.generation_cache_size.unwrap_or(10 * 1024 * 1024);
        let repoid = this.repoid;
        let scuba_table = this.scuba_table;
        let wireproto_scribe_category = this.wireproto_scribe_category;
        let cache_warmup = this.cache_warmup.map(|cache_warmup| CacheWarmupParams {
            bookmark: Bookmark::new(cache_warmup.bookmark).expect("bookmark name must be ascii"),
            commit_limit: cache_warmup.commit_limit.unwrap_or(200000),
        });
        let hook_manager_params = this.hook_manager_params.map(|params| HookManagerParams {
            entrylimit: params.entrylimit,
            weightlimit: params.weightlimit,
        });
        let bookmarks = match this.bookmarks {
            Some(bookmarks) => Some(
                bookmarks
                    .into_iter()
                    .map(|bm| BookmarkParams {
                        bookmark: Bookmark::new(bm.name).unwrap(),
                        hooks: match bm.hooks {
                            Some(hooks) => {
                                Some(hooks.into_iter().map(|rbmh| rbmh.hook_name).collect())
                            }
                            None => None,
                        },
                    })
                    .collect(),
            ),
            None => None,
        };

        let hooks_opt;
        if hooks.len() != 0 {
            hooks_opt = Some(hooks);
        } else {
            hooks_opt = None;
        }

        let pushrebase = this.pushrebase
            .map(|raw| {
                let default = PushrebaseParams::default();
                PushrebaseParams {
                    rewritedates: raw.rewritedates.unwrap_or(default.rewritedates),
                    recursion_limit: raw.recursion_limit.unwrap_or(default.recursion_limit),
                }
            })
            .unwrap_or_default();

        let lfs = match this.lfs {
            Some(lfs_params) => LfsParams {
                threshold: lfs_params.threshold,
            },
            None => LfsParams { threshold: None },
        };

        let hash_validation_percentage = this.hash_validation_percentage.unwrap_or(0);

        let readonly = if this.readonly.unwrap_or(false) {
            RepoReadOnly::ReadOnly
        } else {
            RepoReadOnly::ReadWrite
        };

        let skiplist_index_blobstore_key = this.skiplist_index_blobstore_key;
        Ok(RepoConfig {
            enabled,
            repotype,
            generation_cache_size,
            repoid,
            scuba_table,
            cache_warmup,
            hook_manager_params,
            bookmarks,
            hooks: hooks_opt,
            pushrebase,
            lfs,
            wireproto_scribe_category,
            hash_validation_percentage,
            readonly,
            skiplist_index_blobstore_key,
        })
    }
}

#[derive(Debug, Deserialize, Clone)]
struct RawRepoConfig {
    path: Option<PathBuf>,
    repotype: RawRepoType,
    enabled: Option<bool>,
    generation_cache_size: Option<usize>,
    manifold_bucket: Option<String>,
    manifold_prefix: Option<String>,
    repoid: i32,
    db_address: Option<String>,
    filenode_shards: Option<usize>,
    scuba_table: Option<String>,
    delay_mean: Option<u64>,
    delay_stddev: Option<u64>,
    io_thread_num: Option<usize>,
    cache_warmup: Option<RawCacheWarmupConfig>,
    bookmarks: Option<Vec<RawBookmarkConfig>>,
    hooks: Option<Vec<RawHookConfig>>,
    pushrebase: Option<RawPushrebaseParams>,
    lfs: Option<RawLfsParams>,
    wireproto_scribe_category: Option<String>,
    hash_validation_percentage: Option<usize>,
    readonly: Option<bool>,
    hook_manager_params: Option<HookManagerParams>,
    skiplist_index_blobstore_key: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct RawCacheWarmupConfig {
    bookmark: String,
    commit_limit: Option<usize>,
}

#[derive(Debug, Deserialize, Clone)]
struct RawHookManagerParams {
    entrylimit: usize,
    weightlimit: usize,
}

#[derive(Debug, Deserialize, Clone)]
struct RawBookmarkConfig {
    name: String,
    hooks: Option<Vec<RawBookmarkHook>>,
}

#[derive(Debug, Deserialize, Clone)]
struct RawBookmarkHook {
    hook_name: String,
}

#[derive(Debug, Deserialize, Clone)]
struct RawHookConfig {
    name: String,
    path: Option<String>,
    hook_type: HookType,
    bypass_commit_string: Option<String>,
    bypass_pushvar: Option<String>,
}

/// Types of repositories supported
#[derive(Clone, Debug, Deserialize)]
enum RawRepoType {
    #[serde(rename = "blob:files")] Files,
    #[serde(rename = "blob:rocks")] BlobRocks,
    #[serde(rename = "blob:testmanifold")] TestBlobManifold,
    #[serde(rename = "blob:testdelay")] TestBlobDelayRocks,
}

#[derive(Clone, Debug, Deserialize)]
struct RawPushrebaseParams {
    rewritedates: Option<bool>,
    recursion_limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
struct RawLfsParams {
    threshold: Option<u64>,
}

#[cfg(test)]
mod test {
    use super::*;

    use mercurial_types::FileType;
    use mercurial_types_mocks::manifest::MockManifest;

    #[test]
    fn test_read_manifest() {
        let hook1_content = "this is hook1";
        let hook2_content = "this is hook2";
        let fbsource_content = r#"
            path="/tmp/fbsource"
            repotype="blob:rocks"
            generation_cache_size=1048576
            repoid=0
            scuba_table="scuba_table"
            skiplist_index_blobstore_key="skiplist_key"
            [cache_warmup]
            bookmark="master"
            commit_limit=100
            [hook_manager_params]
            entrylimit=1234
            weightlimit=4321
            [[bookmarks]]
            name="master"
            [[bookmarks.hooks]]
            hook_name="hook1"
            [[bookmarks.hooks]]
            hook_name="hook2"
            [[bookmarks.hooks]]
            hook_name="rust:rusthook"
            [[hooks]]
            name="hook1"
            path="common/hooks/hook1.lua"
            hook_type="PerAddedOrModifiedFile"
            bypass_commit_string="@allow_hook1"
            [[hooks]]
            name="hook2"
            path="./hooks/hook2.lua"
            hook_type="PerChangeset"
            bypass_pushvar="pushvar=pushval"
            [[hooks]]
            name="rust:rusthook"
            hook_type="PerChangeset"
            [pushrebase]
            rewritedates = false
            recursion_limit = 1024
            [lfs]
            threshold = 1000
        "#;
        let www_content = r#"
            path="/tmp/www"
            repotype="blob:files"
            repoid=1
            scuba_table="scuba_table"
            wireproto_scribe_category="category"
        "#;

        let paths = btreemap! {
            "common/hooks/hook1.lua" => (FileType::Regular, hook1_content),
            "repos/fbsource/server.toml" => (FileType::Regular, fbsource_content),
            "repos/fbsource/hooks/hook2.lua" => (FileType::Regular, hook2_content),
            "repos/www/server.toml" => (FileType::Regular, www_content),
            "my_path/my_files" => (FileType::Regular, ""),
        };
        let root_manifest = MockManifest::from_paths(paths).expect("manifest is valid");
        let repoconfig = RepoConfigs::read_manifest(&root_manifest)
            .wait()
            .expect("failed to read config from manifest");

        let mut repos = HashMap::new();
        repos.insert(
            "fbsource".to_string(),
            RepoConfig {
                enabled: true,
                repotype: RepoType::BlobRocks("/tmp/fbsource".into()),
                generation_cache_size: 1024 * 1024,
                repoid: 0,
                scuba_table: Some("scuba_table".to_string()),
                cache_warmup: Some(CacheWarmupParams {
                    bookmark: Bookmark::new("master").unwrap(),
                    commit_limit: 100,
                }),
                hook_manager_params: Some(HookManagerParams {
                    entrylimit: 1234,
                    weightlimit: 4321,
                }),
                bookmarks: Some(vec![
                    BookmarkParams {
                        bookmark: Bookmark::new("master").unwrap(),
                        hooks: Some(vec![
                            "hook1".to_string(),
                            "hook2".to_string(),
                            "rust:rusthook".to_string(),
                        ]),
                    },
                ]),
                hooks: Some(vec![
                    HookParams {
                        name: "hook1".to_string(),
                        code: Some("this is hook1".to_string()),
                        hook_type: HookType::PerAddedOrModifiedFile,
                        bypass: Some(HookBypass::CommitMessage("@allow_hook1".into())),
                    },
                    HookParams {
                        name: "hook2".to_string(),
                        code: Some("this is hook2".to_string()),
                        hook_type: HookType::PerChangeset,
                        bypass: Some(HookBypass::Pushvar {
                            name: "pushvar".into(),
                            value: "pushval".into(),
                        }),
                    },
                    HookParams {
                        name: "rust:rusthook".to_string(),
                        code: None,
                        hook_type: HookType::PerChangeset,
                        bypass: None,
                    },
                ]),
                pushrebase: PushrebaseParams {
                    rewritedates: false,
                    recursion_limit: 1024,
                },
                lfs: LfsParams {
                    threshold: Some(1000),
                },
                wireproto_scribe_category: None,
                hash_validation_percentage: 0,
                readonly: RepoReadOnly::ReadWrite,
                skiplist_index_blobstore_key: Some("skiplist_key".into()),
            },
        );
        repos.insert(
            "www".to_string(),
            RepoConfig {
                enabled: true,
                repotype: RepoType::BlobFiles("/tmp/www".into()),
                generation_cache_size: 10 * 1024 * 1024,
                repoid: 1,
                scuba_table: Some("scuba_table".to_string()),
                cache_warmup: None,
                hook_manager_params: None,
                bookmarks: None,
                hooks: None,
                pushrebase: Default::default(),
                lfs: Default::default(),
                wireproto_scribe_category: Some("category".to_string()),
                hash_validation_percentage: 0,
                readonly: RepoReadOnly::ReadWrite,
                skiplist_index_blobstore_key: None,
            },
        );
        assert_eq!(
            repoconfig,
            RepoConfigs {
                metaconfig: MetaConfig {},
                repos,
            }
        )
    }

    #[test]
    fn test_broken_config() {
        // Two bypasses for one hook
        let hook1_content = "this is hook1";
        let content = r#"
            path="/tmp/fbsource"
            repotype="blob:rocks"
            repoid=0
            [[bookmarks]]
            name="master"
            [[bookmarks.hooks]]
            hook_name="hook1"
            [[hooks]]
            name="hook1"
            path="common/hooks/hook1.lua"
            hook_type="PerAddedOrModifiedFile"
            bypass_commit_string="@allow_hook1"
            bypass_pushvar="var=val"
        "#;

        let paths = btreemap! {
            "common/hooks/hook1.lua" => (FileType::Regular, hook1_content),
            "repos/fbsource/server.toml" => (FileType::Regular, content),
        };
        let root_manifest = MockManifest::from_paths(paths).expect("manifest is valid");
        let res = RepoConfigs::read_manifest(&root_manifest).wait();
        assert!(res.is_err());

        // Incorrect bypass string
        let hook1_content = "this is hook1";
        let content = r#"
            path="/tmp/fbsource"
            repotype="blob:rocks"
            repoid=0
            [[bookmarks]]
            name="master"
            [[bookmarks.hooks]]
            hook_name="hook1"
            [[hooks]]
            name="hook1"
            path="common/hooks/hook1.lua"
            hook_type="PerAddedOrModifiedFile"
            bypass_pushvar="var"
        "#;

        let paths = btreemap! {
            "common/hooks/hook1.lua" => (FileType::Regular, hook1_content),
            "repos/fbsource/server.toml" => (FileType::Regular, content),
        };
        let root_manifest = MockManifest::from_paths(paths).expect("manifest is valid");
        let res = RepoConfigs::read_manifest(&root_manifest).wait();
        assert!(res.is_err());
    }
}

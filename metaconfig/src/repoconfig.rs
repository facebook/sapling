// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Contains structures describing configuration of the entire repo. Those structures are
//! deserialized from TOML files from metaconfig repo

use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::path::PathBuf;
use std::str::from_utf8;

use failure::FutureFailureErrorExt;
use futures::{future, Future, IntoFuture};

use blobrepo::{BlobRepo, ManifoldArgs};
use bookmarks::Bookmark;
use mercurial_types::{Changeset, MPath, MPathElement, Manifest};
use mercurial_types::manifest::Content;
use mercurial_types::nodehash::HgChangesetId;
use mononoke_types::FileContents;
use toml;
use vfs::{vfs_from_manifest, ManifestVfsDir, ManifestVfsFile, VfsDir, VfsFile, VfsNode, VfsWalker};

use errors::*;

/// Configuration of a single repository
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RepoConfig {
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

/// Configuration for a bookmark
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BookmarkParams {
    /// The name of the bookmark
    pub name: String,
    /// The hooks active for the bookmark
    pub hooks: Option<Vec<String>>,
}

/// Configuration for a hook
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookParams {
    /// The name of the hook
    pub name: String,
    /// The path to the hook
    pub path: String,
}

/// Types of repositories supported
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RepoType {
    /// Revlog repository with path pointing to on-disk checkout of repository
    Revlog(PathBuf),
    /// Blob repository with path pointing to on-disk files with data. The files are stored in a
    /// RocksDb database
    BlobRocks(PathBuf),
    /// Blob repository with path pointing to the directory where a server socket is going to be.
    BlobManifold {
        /// The arguments used to connect to Manifold.
        args: ManifoldArgs,
        /// Path is used to connect Mononoke server to hgcli
        path: PathBuf,
    },
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
                    repo.get_manifest_by_nodeid(&changeset.manifestid().clone().into_nodehash())
                })
                .context("failed to get manifest from changeset")
                .from_err()
                .and_then(|manifest| Self::read_manifest(&manifest)),
        )
    }

    /// Read the given manifest of metaconfig repo and yield the RepoConfigs for it
    fn read_manifest<M>(manifest: &M) -> Box<Future<Item = Self, Error = Error> + Send>
    where
        M: Manifest,
    {
        Box::new(
            vfs_from_manifest(manifest)
                .and_then(|vfs| {
                    VfsWalker::new(vfs.into_node(), MPath::new(b"repos").unwrap()).walk()
                })
                .from_err()
                .and_then(|repos_node| match repos_node {
                    VfsNode::File(_) => {
                        bail_err!(ErrorKind::InvalidFileStructure("expected directory".into()))
                    }
                    VfsNode::Dir(dir) => Ok(dir),
                })
                .and_then(|repos_dir| {
                    let repopaths: Vec<_> = repos_dir.read().into_iter().cloned().collect();
                    let repos_node = repos_dir.into_node();
                    future::join_all(
                        repopaths
                            .into_iter()
                            .map(move |repopath| Self::read_repo(repos_node.clone(), repopath)),
                    )
                })
                .map(|repos| RepoConfigs {
                    metaconfig: MetaConfig {},
                    repos: repos.into_iter().collect(),
                }),
        )
    }

    fn read_repo(
        dir: VfsNode<ManifestVfsDir, ManifestVfsFile>,
        path: MPathElement,
    ) -> Box<Future<Item = (String, RepoConfig), Error = Error> + Send> {
        Box::new(
            from_utf8(path.as_bytes())
                .map(ToOwned::to_owned)
                .into_future()
                .from_err()
                .and_then({
                    let path = path.clone();
                    move |reponame| {
                        VfsWalker::new(dir, path.into_iter().cloned())
                            .walk()
                            .from_err()
                            .and_then(|node| match node {
                                VfsNode::File(file) => Ok(file),
                                _ => Err(ErrorKind::InvalidFileStructure("expected file".into()).into()),
                            })
                            .and_then(|file| {
                                file.read().map_err(|err| {
                                    err.context("failed to read content of the file").into()
                                })
                            })
                            .and_then(|content| match content {
                                Content::File(FileContents::Bytes(bytes)) => Ok(bytes),
                                _ => Err(ErrorKind::InvalidFileStructure("expected file".into()).into()),
                            })
                            .and_then(|bytes| {
                                Ok((
                                    reponame,
                                    toml::from_slice::<RawRepoConfig>(bytes.as_ref())?.try_into()?,
                                ))
                            })
                    }
                })
                .map_err(move |err: Error| {
                    err.context(format_err!("failed while parsing file: {:?}", path))
                        .into()
                }),
        )
    }
}

#[derive(Debug, Deserialize)]
struct RawRepoConfig {
    path: PathBuf,
    repotype: RawRepoType,
    generation_cache_size: Option<usize>,
    manifold_bucket: Option<String>,
    manifold_prefix: Option<String>,
    repoid: i32,
    db_address: Option<String>,
    scuba_table: Option<String>,
    delay_mean: Option<u64>,
    delay_stddev: Option<u64>,
    blobstore_cache_size: Option<usize>,
    changesets_cache_size: Option<usize>,
    filenodes_cache_size: Option<usize>,
    io_thread_num: Option<usize>,
    cache_warmup: Option<RawCacheWarmupConfig>,
    max_concurrent_requests_per_io_thread: Option<usize>,
    bookmarks: Option<Vec<RawBookmarkConfig>>,
    hooks: Option<Vec<RawHookConfig>>,
}

#[derive(Debug, Deserialize)]
struct RawCacheWarmupConfig {
    bookmark: String,
    commit_limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct RawBookmarkConfig {
    name: String,
    hooks: Option<Vec<RawBookmarkHook>>,
}

#[derive(Debug, Deserialize)]
struct RawBookmarkHook {
    hook_name: String,
}

#[derive(Debug, Deserialize)]
struct RawHookConfig {
    name: String,
    path: String,
}

/// Types of repositories supported
#[derive(Clone, Debug, Deserialize)]
enum RawRepoType {
    #[serde(rename = "revlog")] Revlog,
    #[serde(rename = "blob:rocks")] BlobRocks,
    #[serde(rename = "blob:testmanifold")] BlobManifold,
    #[serde(rename = "blob:testdelay")] TestBlobDelayRocks,
}

impl TryFrom<RawRepoConfig> for RepoConfig {
    type Error = Error;

    fn try_from(this: RawRepoConfig) -> Result<Self> {
        use self::RawRepoType::*;

        let repotype = match this.repotype {
            Revlog => RepoType::Revlog(this.path),
            BlobRocks => RepoType::BlobRocks(this.path),
            BlobManifold => {
                let manifold_bucket = this.manifold_bucket.ok_or(ErrorKind::InvalidConfig(
                    "manifold bucket must be specified".into(),
                ))?;
                let db_address = this.db_address.expect("xdb tier was not specified");
                RepoType::BlobManifold {
                    args: ManifoldArgs {
                        bucket: manifold_bucket,
                        prefix: this.manifold_prefix.unwrap_or("".into()),
                        db_address,
                        blobstore_cache_size: this.blobstore_cache_size.unwrap_or(100_000_000),
                        changesets_cache_size: this.changesets_cache_size.unwrap_or(100_000_000),
                        filenodes_cache_size: this.filenodes_cache_size.unwrap_or(100_000_000),
                        io_threads: this.io_thread_num.unwrap_or(5),
                        max_concurrent_requests_per_io_thread:
                            this.max_concurrent_requests_per_io_thread.unwrap_or(4),
                    },
                    path: this.path,
                }
            }
            TestBlobDelayRocks => RepoType::TestBlobDelayRocks(
                this.path,
                this.delay_mean.expect("mean delay must be specified"),
                this.delay_stddev.expect("stddev delay must be specified"),
            ),
        };

        let generation_cache_size = this.generation_cache_size.unwrap_or(10 * 1024 * 1024);
        let repoid = this.repoid;
        let scuba_table = this.scuba_table;
        let cache_warmup = this.cache_warmup.map(|cache_warmup| CacheWarmupParams {
            bookmark: Bookmark::new(cache_warmup.bookmark).expect("bookmark name must be ascii"),
            commit_limit: cache_warmup.commit_limit.unwrap_or(200000),
        });
        let bookmarks = match this.bookmarks {
            Some(bm) => Some(
                bm.into_iter()
                    .map(|bm| BookmarkParams {
                        name: bm.name,
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
        let hooks = match this.hooks {
            Some(hook) => Some(
                hook.into_iter()
                    .map(|hook| HookParams {
                        name: hook.name,
                        path: hook.path,
                    })
                    .collect(),
            ),
            None => None,
        };

        Ok(RepoConfig {
            repotype,
            generation_cache_size,
            repoid,
            scuba_table,
            cache_warmup,
            bookmarks,
            hooks,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use mercurial_types::FileType;
    use mercurial_types_mocks::manifest::MockManifest;

    #[test]
    fn test_read_manifest() {
        let fbsource_content = r#"
            path="/tmp/fbsource"
            repotype="blob:rocks"
            generation_cache_size=1048576
            repoid=0
            scuba_table="scuba_table"
            [cache_warmup]
            bookmark="master"
            commit_limit=100
            [[bookmarks]]
            name="bookmark_fbs1"
            [[bookmarks.hooks]]
            hook_name="hook_fbs1"
            [[bookmarks.hooks]]
            hook_name="hook_fbs2"
            [[bookmarks]]
            name="bookmark_fbs2"
            [[hooks]]
            name="hook_fbs1"
            path="blah/hooks/hook_fbs1.lua"
            [[hooks]]
            name="hook_fbs2"
            path="blah/hooks/hook_fbs2.lua"
        "#;
        let www_content = r#"
            path="/tmp/www"
            repotype="revlog"
            repoid=1
            scuba_table="scuba_table"
        "#;

        let paths = btreemap! {
            "repos/fbsource" => (FileType::Regular, fbsource_content),
            "repos/www" => (FileType::Regular, www_content),
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
                repotype: RepoType::BlobRocks("/tmp/fbsource".into()),
                generation_cache_size: 1024 * 1024,
                repoid: 0,
                scuba_table: Some("scuba_table".to_string()),
                cache_warmup: Some(CacheWarmupParams {
                    bookmark: Bookmark::new("master").unwrap(),
                    commit_limit: 100,
                }),
                bookmarks: Some(vec![
                    BookmarkParams {
                        name: "bookmark_fbs1".to_string(),
                        hooks: Some(vec!["hook_fbs1".to_string(), "hook_fbs2".to_string()]),
                    },
                    BookmarkParams {
                        name: "bookmark_fbs2".to_string(),
                        hooks: None,
                    },
                ]),
                hooks: Some(vec![
                    HookParams {
                        name: "hook_fbs1".to_string(),
                        path: "blah/hooks/hook_fbs1.lua".to_string(),
                    },
                    HookParams {
                        name: "hook_fbs2".to_string(),
                        path: "blah/hooks/hook_fbs2.lua".to_string(),
                    },
                ]),
            },
        );
        repos.insert(
            "www".to_string(),
            RepoConfig {
                repotype: RepoType::Revlog("/tmp/www".into()),
                generation_cache_size: 10 * 1024 * 1024,
                repoid: 1,
                scuba_table: Some("scuba_table".to_string()),
                cache_warmup: None,
                bookmarks: None,
                hooks: None,
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
}

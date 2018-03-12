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

use futures::{future, Future, IntoFuture};

use blobrepo::BlobRepo;
use mercurial::RevlogRepo;
use mercurial_types::{Changeset, MPath, MPathElement, Manifest};
use mercurial_types::manifest::Content;
use mercurial_types::nodehash::ChangesetId;
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
}

/// Types of repositories supported
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RepoType {
    /// Revlog repository with path pointing to on-disk checkout of repository
    Revlog(PathBuf),
    /// Blob repository with path pointing to on-disk files with data
    BlobFiles(PathBuf),
    /// Blob repository with path pointing to on-disk files with data. The files are stored in a
    /// RocksDb database
    BlobRocks(PathBuf),
    /// Blob repository with path pointing to the directory where a server socket is going to be.
    /// Blobs are stored in Manifold, first parameter is Manifold bucket, second is prefix.
    /// Bookmarks and heads are stored in memory
    TestBlobManifold(String, String, PathBuf),
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
        changesetid: ChangesetId,
    ) -> Box<Future<Item = Self, Error = Error> + Send> {
        Box::new(
            repo.get_changeset_by_changesetid(&changesetid)
                .and_then(move |changeset| {
                    repo.get_manifest_by_nodeid(&changeset.manifestid().clone().into_nodehash())
                })
                .map_err(|err| err.context("failed to get manifest from changeset").into())
                .and_then(|manifest| Self::read_manifest(&manifest)),
        )
    }

    /// Read the config repo and generate RepoConfigs based on it
    pub fn read_revlog_config_repo(
        repo: RevlogRepo,
        changesetid: ChangesetId,
    ) -> Box<Future<Item = Self, Error = Error> + Send> {
        Box::new(
            repo.get_changeset_by_changesetid(&changesetid)
                .and_then(move |changeset| {
                    repo.get_manifest_by_nodeid(&changeset.manifestid().clone().into_nodehash())
                })
                .map_err(|err| err.context("failed to get manifest from changeset").into())
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
                                Content::File(blob) => Ok(blob),
                                _ => Err(ErrorKind::InvalidFileStructure("expected file".into()).into()),
                            })
                            .and_then(|blob| {
                                let bytes =
                                    blob.as_slice().ok_or(ErrorKind::InvalidFileStructure(
                                        "expected content of the blob".into(),
                                    ))?;
                                Ok((
                                    reponame,
                                    toml::from_slice::<RawRepoConfig>(bytes)?.try_into()?,
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
    scuba_table: Option<String>,
}

/// Types of repositories supported
#[derive(Clone, Debug, Deserialize)]
enum RawRepoType {
    #[serde(rename = "revlog")] Revlog,
    #[serde(rename = "blob:files")] BlobFiles,
    #[serde(rename = "blob:rocks")] BlobRocks,
    #[serde(rename = "blob:testmanifold")] TestBlobManifold,
}

impl TryFrom<RawRepoConfig> for RepoConfig {
    type Error = Error;

    fn try_from(this: RawRepoConfig) -> Result<Self> {
        use self::RawRepoType::*;

        let repotype = match this.repotype {
            Revlog => RepoType::Revlog(this.path),
            BlobFiles => RepoType::BlobFiles(this.path),
            BlobRocks => RepoType::BlobRocks(this.path),
            TestBlobManifold => {
                let manifold_bucket = this.manifold_bucket.ok_or(ErrorKind::InvalidConfig(
                    "manifold bucket must be specified".into(),
                ))?;
                RepoType::TestBlobManifold(
                    manifold_bucket,
                    this.manifold_prefix.unwrap_or("".into()),
                    this.path,
                )
            }
        };

        let generation_cache_size = this.generation_cache_size.unwrap_or(10 * 1024 * 1024);
        let repoid = this.repoid;
        let scuba_table = this.scuba_table;

        Ok(RepoConfig {
            repotype,
            generation_cache_size,
            repoid,
            scuba_table,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use std::sync::Arc;

    use mercurial_types::Type;
    use mercurial_types_mocks::manifest::{make_file, MockManifest};

    #[test]
    fn test_read_manifest() {
        let fbsource_content = r#"
            path="/tmp/fbsource"
            repotype="blob:files"
            generation_cache_size=1048576
            repoid=0
            scuba_table="scuba_table"
        "#;
        let www_content = r#"
            path="/tmp/www"
            repotype="revlog"
            repoid=1
            scuba_table="scuba_table"
        "#;

        let my_path_manifest = MockManifest::with_content(vec![
            ("my_files", Arc::new(|| unimplemented!()), Type::File),
        ]);

        let repos_manifest = MockManifest::with_content(vec![
            ("fbsource", make_file(fbsource_content), Type::File),
            ("www", make_file(www_content), Type::File),
        ]);

        let repoconfig = RepoConfigs::read_manifest(&MockManifest::with_content(vec![
            (
                "my_path",
                Arc::new(move || Content::Tree(Box::new(my_path_manifest.clone()))),
                Type::File,
            ),
            (
                "repos",
                Arc::new(move || Content::Tree(Box::new(repos_manifest.clone()))),
                Type::Tree,
            ),
        ])).wait()
            .expect("failed to read config from manifest");

        let mut repos = HashMap::new();
        repos.insert(
            "fbsource".to_string(),
            RepoConfig {
                repotype: RepoType::BlobFiles("/tmp/fbsource".into()),
                generation_cache_size: 1024 * 1024,
                repoid: 0,
                scuba_table: Some("scuba_table".to_string()),
            },
        );
        repos.insert(
            "www".to_string(),
            RepoConfig {
                repotype: RepoType::Revlog("/tmp/www".into()),
                generation_cache_size: 10 * 1024 * 1024,
                repoid: 1,
                scuba_table: Some("scuba_table".to_string()),
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

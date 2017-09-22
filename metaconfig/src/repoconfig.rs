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

use error_chain::ChainedError;

use mercurial::file::File;
use mercurial_types::{BlobNode, Manifest, NodeHash, Path, Repo};
use mercurial_types::manifest::Content;
use mercurial_types::path::PathElement;
use toml;
use vfs::{vfs_from_manifest, ManifestVfsDir, ManifestVfsFile, VfsDir, VfsFile, VfsNode, VfsWalker};

use errors::*;

/// Configuration of a single repository
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RepoConfig {
    /// Defines the type of repository
    pub repotype: RepoType,
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
    // BlobManifold...
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
    pub fn read_config_repo<E>(
        repo: Box<Repo<Error = E> + Send>,
        changeset_hash: NodeHash,
    ) -> Box<Future<Item = Self, Error = Error> + Send>
    where
        E: Send + 'static + ::std::error::Error,
    {
        Box::new(
            repo.get_changeset_by_nodeid(&changeset_hash)
                .and_then(move |changeset| {
                    repo.get_manifest_by_nodeid(changeset.manifestid())
                })
                .map_err(|err| {
                    ChainedError::with_chain(err, "failed to get manifest from changeset")
                })
                .and_then(|manifest| Self::read_manifest(&manifest)),
        )
    }

    /// Read the given manifest of metaconfig repo and yield the RepoConfigs for it
    fn read_manifest<M, E>(manifest: &M) -> Box<Future<Item = Self, Error = Error> + Send>
    where
        M: Manifest<Error = E>,
        E: Send + 'static + ::std::error::Error,
    {
        Box::new(
            vfs_from_manifest(manifest)
                .and_then(|vfs| {
                    VfsWalker::new(vfs.into_node(), Path::new(b"repos").unwrap()).walk()
                })
                .from_err()
                .and_then(|repos_node| match repos_node {
                    VfsNode::File(_) => Err(
                        ErrorKind::InvalidFileStructure("expected directory".into()).into(),
                    ),
                    VfsNode::Dir(dir) => Ok(dir),
                })
                .and_then(|repos_dir| {
                    let repopaths: Vec<_> = repos_dir.read().into_iter().cloned().collect();
                    let repos_node = repos_dir.into_node();
                    future::join_all(repopaths.into_iter().map(move |repopath| {
                        Self::read_repo(repos_node.clone(), repopath)
                    }))
                })
                .map(|repos| {
                    RepoConfigs {
                        metaconfig: MetaConfig {},
                        repos: repos.into_iter().collect(),
                    }
                }),
        )
    }

    fn read_repo<E>(
        dir: VfsNode<ManifestVfsDir<E>, ManifestVfsFile<E>>,
        path: PathElement,
    ) -> Box<Future<Item = (String, RepoConfig), Error = Error> + Send>
    where
        E: Send + 'static + ::std::error::Error,
    {
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
                                _ => Err(
                                    ErrorKind::InvalidFileStructure("expected file".into()).into(),
                                ),
                            })
                            .and_then(|file| {
                                file.read().map_err(|err| {
                                    ChainedError::with_chain(
                                        err,
                                        "failed to read content of the file",
                                    )
                                })
                            })
                            .and_then(|content| match content {
                                Content::File(blob) => Ok(blob),
                                _ => Err(
                                    ErrorKind::InvalidFileStructure("expected file".into()).into(),
                                ),
                            })
                            .and_then(|blob| {
                                let file = File::new(BlobNode::new(blob, None, None));
                                let bytes = file.content().ok_or_else(|| {
                                    ErrorKind::InvalidFileStructure(
                                        "expected content of the blob".into(),
                                    )
                                })?;
                                Ok((
                                    reponame,
                                    toml::from_slice::<RawRepoConfig>(bytes)
                                        .map_err(ErrorKind::De)?
                                        .try_into()?,
                                ))
                            })
                    }
                })
                .map_err(move |err: Error| {
                    err.chain_err(|| format!("failed while parsing file: {:?}", path))
                }),
        )
    }
}

#[derive(Debug, Deserialize)]
struct RawRepoConfig {
    path: PathBuf,
    repotype: RawRepoType,
}

/// Types of repositories supported
#[derive(Clone, Debug, Deserialize)]
enum RawRepoType {
    #[serde(rename = "revlog")] Revlog,
    #[serde(rename = "blob:files")] BlobFiles,
    #[serde(rename = "blob:rocks")] BlobRocks,
}

impl TryFrom<RawRepoConfig> for RepoConfig {
    type Error = Error;

    fn try_from(this: RawRepoConfig) -> Result<Self> {
        use self::RawRepoType::*;

        let repotype = match this.repotype {
            Revlog => RepoType::Revlog(this.path),
            BlobFiles => RepoType::BlobFiles(this.path),
            BlobRocks => RepoType::BlobRocks(this.path),
        };

        Ok(RepoConfig { repotype })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use std::sync::Arc;

    use mercurial_types_mocks::manifest::{make_file, MockManifest};

    #[test]
    fn test_read_manifest() {
        let fbsource_content = r#"
            path="/tmp/fbsource"
            repotype="blob:files"
        "#;
        let www_content = r#"
            path="/tmp/www"
            repotype="revlog"
        "#;

        let repoconfig = RepoConfigs::read_manifest(&MockManifest::<Error>::with_content(vec![
            ("my_path/my_files", Arc::new(|| unimplemented!())),
            ("repos/fbsource", make_file(fbsource_content)),
            ("repos/www", make_file(www_content)),
        ])).wait()
            .expect("failed to read config from manifest");

        let mut repos = HashMap::new();
        repos.insert(
            "fbsource".to_string(),
            RepoConfig {
                repotype: RepoType::BlobFiles("/tmp/fbsource".into()),
            },
        );
        repos.insert(
            "www".to_string(),
            RepoConfig {
                repotype: RepoType::Revlog("/tmp/www".into()),
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

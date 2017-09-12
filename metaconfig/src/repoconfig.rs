// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::str::from_utf8;

use futures::{future, Future, IntoFuture};

use error_chain::ChainedError;

use mercurial_types::{Blob, Manifest, Path};
use mercurial_types::manifest::Content;
use mercurial_types::path::PathElement;
use toml;
use vfs::{vfs_from_manifest, ManifestVfsDir, ManifestVfsFile, VfsDir, VfsFile, VfsNode, VfsWalker};

use errors::*;

/// Holds configuration all configuration that was read from metaconfig repository's manifest.
/// Contains both the configuration for metaconfig repo itself as well as configs of other repos
#[derive(Debug, PartialEq)]
pub struct RepoConfigs {
    metaconfig: RepoConfig,
    repos: HashMap<String, RepoConfig>,
}

/// Configuration of a single repository be it metaconfig or regular one.
#[derive(Debug, PartialEq, Deserialize)]
pub struct RepoConfig {
    version: usize,
}

impl RepoConfigs {
    /// Read the given manifest of metaconfig repo and yield the RepoConfigs for it.
    pub fn read<M, E>(manifest: &M) -> Box<Future<Item = Self, Error = Error> + Send>
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
                        ErrorKind::InvalidFileStructure("expected file".into()).into(),
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
                        metaconfig: RepoConfig { version: 0 },
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
                .and_then(move |reponame| {
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
                                ChainedError::with_chain(err, "failed to read content of the file")
                            })
                        })
                        .and_then(|content| match content {
                            Content::File(Blob::Dirty(bytes)) => Ok(bytes),
                            _ => Err(
                                ErrorKind::InvalidFileStructure("expected dirty blob".into())
                                    .into(),
                            ),
                        })
                        .and_then(|bytes| {
                            Ok((
                                reponame,
                                toml::from_slice::<RepoConfig>(&bytes).map_err(ErrorKind::De)?,
                            ))
                        })
                }),
        )
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use std::sync::Arc;

    use mercurial_types_mocks::manifest::{make_file, MockManifest};

    #[test]
    fn test_empty_repoconfigs() {
        let repoconfig = RepoConfigs::read(&MockManifest::<Error>::with_content(vec![
            ("my_path/my_files", Arc::new(|| unimplemented!())),
            ("repos/www", make_file("version=1")),
            ("repos/fbsource", make_file("version=2")),
        ])).wait()
            .expect("failed to read config from manifest");

        let mut repos = HashMap::new();
        repos.insert("fbsource".to_string(), RepoConfig { version: 2 });
        repos.insert("www".to_string(), RepoConfig { version: 1 });
        assert_eq!(
            repoconfig,
            RepoConfigs {
                metaconfig: RepoConfig { version: 0 },
                repos,
            }
        )
    }
}

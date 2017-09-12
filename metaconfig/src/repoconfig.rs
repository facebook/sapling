// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::str::from_utf8;

use futures::Future;

use mercurial_types::{Manifest, Path};
use vfs::{vfs_from_manifest, VfsDir, VfsNode, VfsWalker};

use errors::*;

/// Contains both the configuration for metaconfig repo itself as well as configs of other repos
#[derive(Debug, PartialEq)]
pub struct RepoConfigs {
    metaconfig: RepoConfig,
    repos: HashMap<String, RepoConfig>,
}

/// Configuration of a single repository be it metaconfig or regular one.
#[derive(Debug, PartialEq)]
pub struct RepoConfig;

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
                .map(|repos_dir| match repos_dir {
                    VfsNode::File(_) => HashMap::new(),
                    VfsNode::Dir(dir) => dir.read()
                        .into_iter()
                        .map(|reponame| {
                            (
                                from_utf8(reponame.as_bytes())
                                    .expect(&format!("invalid unicode in {:?}", reponame))
                                    .to_string(),
                                RepoConfig,
                            )
                        })
                        .collect(),
                })
                .map(|repos| {
                    RepoConfigs {
                        metaconfig: RepoConfig,
                        repos,
                    }
                }),
        )
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use mercurial_types_mocks::manifest::MockManifest;

    #[test]
    fn test_empty_repoconfigs() {
        let repoconfig = RepoConfigs::read(&MockManifest::<Error>::new(
            vec!["my_path/my_files", "repos/www", "repos/fbsource"],
        )).wait()
            .expect("failed to read config from manifest");

        let mut repos = HashMap::new();
        repos.insert("fbsource".to_string(), RepoConfig);
        repos.insert("www".to_string(), RepoConfig);
        assert_eq!(
            repoconfig,
            RepoConfigs {
                metaconfig: RepoConfig,
                repos,
            }
        )
    }
}

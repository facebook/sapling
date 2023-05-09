/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use configmodel::Config;
use configmodel::ConfigExt;
use manifest::DiffType;
use manifest::Manifest;
use manifest_tree::Diff;
use manifest_tree::TreeManifest;
use pathmatcher::AlwaysMatcher;
use storemodel::futures::StreamExt;
use storemodel::ReadFileContents;
use types::Key;
use types::RepoPath;
use types::RepoPathBuf;

use crate::error::CopyTraceError;
use crate::utils::file_path_similarity;
use crate::SearchDirection;

/// Finding rename between old and new trees (commits).
/// old_tree is a parent of new_tree
#[async_trait]
pub trait RenameFinder {
    /// Find the new path of the given old path in the new_tree
    async fn find_rename_forward(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
        old_path: &RepoPath,
    ) -> Result<Option<RepoPathBuf>>;

    /// Find the old path of the given new path in the old_tree
    async fn find_rename_backward(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
        new_path: &RepoPath,
    ) -> Result<Option<RepoPathBuf>>;
}

/// Rename finder for Sapling repo.
pub struct SaplingRenameFinder {
    // Read content and rename metadata of a file
    file_reader: Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync>,
    // Read configs
    config: Arc<dyn Config + Send + Sync>,
}

/// Content similarity based Rename finder (mainly for Git repo)
pub struct ContentSimilarityRenameFinder {
    // Read content and rename metadata of a file
    file_reader: Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync>,
    // Read configs
    config: Arc<dyn Config + Send + Sync>,
}

impl SaplingRenameFinder {
    pub fn new(
        file_reader: Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync>,
        config: Arc<dyn Config + Send + Sync>,
    ) -> Self {
        Self {
            file_reader,
            config,
        }
    }

    async fn read_renamed_metadata_forward(
        &self,
        keys: Vec<Key>,
        old_path: &RepoPath,
    ) -> Result<Option<RepoPathBuf>> {
        tracing::trace!(keys_len = keys.len(), " read_renamed_metadata_forward");
        let mut renames = self.file_reader.read_rename_metadata(keys).await;
        while let Some(rename) = renames.next().await {
            let (key, rename_from_key) = rename?;
            if let Some(rename_from_key) = rename_from_key {
                if rename_from_key.path.as_repo_path() == old_path {
                    return Ok(Some(key.path));
                }
            }
        }
        Ok(None)
    }

    async fn read_renamed_metadata_backward(&self, key: Key) -> Result<Option<RepoPathBuf>> {
        let mut renames = self.file_reader.read_rename_metadata(vec![key]).await;
        if let Some(rename) = renames.next().await {
            let (_, rename_from_key) = rename?;
            return Ok(rename_from_key.map(|k| k.path));
        }
        Ok(None)
    }
}

#[async_trait]
impl RenameFinder for SaplingRenameFinder {
    async fn find_rename_forward(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
        old_path: &RepoPath,
    ) -> Result<Option<RepoPathBuf>> {
        let mut new_files = Vec::new();
        {
            // this block is for dropping `matcher` and `diff` at the end of the block,
            // otherwise the compiler compilains variable might be used across 'await'
            let matcher = AlwaysMatcher::new();
            let diff = Diff::new(old_tree, new_tree, &matcher)?;
            for entry in diff {
                let entry = entry?;

                if let DiffType::RightOnly(file_metadata) = entry.diff_type {
                    let path = entry.path;
                    let key = Key {
                        path,
                        hgid: file_metadata.hgid,
                    };
                    new_files.push(key);
                }
            }
        }

        let candidates = select_rename_candidates(new_files, old_path, &self.config)?;
        self.read_renamed_metadata_forward(candidates, old_path)
            .await
    }

    async fn find_rename_backward(
        &self,
        _old_tree: &TreeManifest,
        new_tree: &TreeManifest,
        new_path: &RepoPath,
    ) -> Result<Option<RepoPathBuf>> {
        let new_key = match new_tree.get_file(new_path)? {
            Some(file_metadata) => Key {
                path: new_path.to_owned(),
                hgid: file_metadata.hgid,
            },
            None => return Ok(None),
        };
        self.read_renamed_metadata_backward(new_key).await
    }
}

impl ContentSimilarityRenameFinder {
    pub fn new(
        file_reader: Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync>,
        config: Arc<dyn Config + Send + Sync>,
    ) -> Self {
        Self {
            file_reader,
            config,
        }
    }

    async fn find_similar_file(
        &self,
        keys: Vec<Key>,
        source_key: Key,
    ) -> Result<Option<RepoPathBuf>> {
        let mut source = self
            .file_reader
            .read_file_contents(vec![source_key.clone()])
            .await;
        let source_content = match source.next().await {
            None => return Err(CopyTraceError::FileNotFound(source_key.path).into()),
            Some(content_and_key) => content_and_key?.0,
        };

        let mut candidates = self.file_reader.read_file_contents(keys).await;
        while let Some(candidate) = candidates.next().await {
            let (candidate_content, k) = candidate?;
            // todo(zhaolong): support more sophisticated content similarity check
            if candidate_content == source_content {
                return Ok(Some(k.path));
            }
        }

        Ok(None)
    }

    async fn find_rename_in_direction(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
        source_path: &RepoPath,
        direction: SearchDirection,
    ) -> Result<Option<RepoPathBuf>> {
        tracing::trace!(?source_path, ?direction, " find_rename_in_direction");
        let mut candidates = Vec::new();
        {
            // this block is for dropping `matcher` and `diff` at the end of the block,
            let matcher = AlwaysMatcher::new();
            let diff = Diff::new(old_tree, new_tree, &matcher)?;
            if direction == SearchDirection::Forward {
                for entry in diff {
                    let entry = entry?;
                    if let DiffType::RightOnly(file_metadata) = entry.diff_type {
                        let key = Key {
                            path: entry.path,
                            hgid: file_metadata.hgid,
                        };
                        candidates.push(key);
                    }
                }
            } else {
                for entry in diff {
                    let entry = entry?;
                    if let DiffType::LeftOnly(file_metadata) = entry.diff_type {
                        let key = Key {
                            path: entry.path,
                            hgid: file_metadata.hgid,
                        };
                        candidates.push(key);
                    }
                }
            }
        }

        let candidates = select_rename_candidates(candidates, source_path, &self.config)?;
        tracing::trace!(candidates_len = candidates.len(), " found");

        let source_tree = match direction {
            SearchDirection::Forward => old_tree,
            SearchDirection::Backward => new_tree,
        };
        let source = match source_tree.get_file(source_path)? {
            None => return Err(CopyTraceError::FileNotFound(source_path.to_owned()).into()),
            Some(file_metadata) => Key {
                path: source_path.to_owned(),
                hgid: file_metadata.hgid,
            },
        };

        self.find_similar_file(candidates, source).await
    }
}

pub(crate) fn select_rename_candidates(
    mut candidates: Vec<Key>,
    source_path: &RepoPath,
    config: &dyn Config,
) -> Result<Vec<Key>> {
    // It's rare that a file will be copied and renamed (multiple copies) in one commit.
    // We don't plan to support this one-to-many mapping since it will make copytrace
    // complexity increase exponentially. Here, we order the potential new files in
    // path similarity order (most similar one first), and return the first one that
    // is a copy of the old_path.
    candidates.sort_by_key(|k| {
        let path = k.path.as_repo_path();
        let score = file_path_similarity(path, source_path);
        (-score, path.to_owned())
    });
    let max_rename_candidates = config
        .get_opt::<usize>("copytrace", "max-rename-candidates")?
        .unwrap_or(10);
    if candidates.len() > max_rename_candidates {
        Ok(candidates.into_iter().take(max_rename_candidates).collect())
    } else {
        Ok(candidates)
    }
}

#[async_trait]
impl RenameFinder for ContentSimilarityRenameFinder {
    async fn find_rename_forward(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
        old_path: &RepoPath,
    ) -> Result<Option<RepoPathBuf>> {
        self.find_rename_in_direction(old_tree, new_tree, old_path, SearchDirection::Forward)
            .await
    }

    async fn find_rename_backward(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
        new_path: &RepoPath,
    ) -> Result<Option<RepoPathBuf>> {
        self.find_rename_in_direction(old_tree, new_tree, new_path, SearchDirection::Backward)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use types::HgId;

    use super::*;

    #[test]
    fn test_select_rename_candidates() {
        let candidates: Vec<Key> = vec![
            gen_key("a/b/c.txt"),
            gen_key("a/b/c.md"),
            gen_key("a/d.txt"),
            gen_key("e.txt"),
        ];
        let source_path = &RepoPath::from_str("a/c.txt").unwrap();
        let mut config: BTreeMap<&'static str, &'static str> = Default::default();
        config.insert("copytrace.max-rename-candidates", "2");
        let config = Arc::new(config);

        let actual = select_rename_candidates(candidates, source_path, &config).unwrap();

        let expected = vec![gen_key("a/b/c.txt"), gen_key("a/d.txt")];
        assert_eq!(actual, expected)
    }

    fn gen_key(path: &str) -> Key {
        let path = RepoPath::from_str(path).unwrap().to_owned();
        let hgid = HgId::null_id().clone();
        Key { path, hgid }
    }
}

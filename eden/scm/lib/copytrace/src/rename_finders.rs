/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::iter::zip;
use std::sync::Arc;

use anyhow::Result;
use async_runtime::block_in_place;
use async_runtime::spawn_blocking;
use async_trait::async_trait;
use configmodel::Config;
use configmodel::ConfigExt;
use dag::Vertex;
use hg_metrics::increment_counter;
use lru_cache::LruCache;
use manifest::DiffType;
use manifest::Manifest;
use manifest_tree::Diff;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use pathmatcher::AlwaysMatcher;
use storemodel::FileStore;
use types::Key;
use types::RepoPath;
use types::RepoPathBuf;

use crate::error::CopyTraceError;
use crate::utils::file_path_similarity;
use crate::utils::is_content_similar;
use crate::SearchDirection;

/// Maximum rename candidate files to check
const DEFAULT_MAX_RENAME_CANDIDATES: usize = 1000;
/// Control if MetadataRenameFinder fallbacks to content similarity finder
const DEFAULT_FALLBACK_TO_CONTENT_SIMILARITY: bool = false;
/// Default Rename cache size
const DEFAULT_RENAME_CACHE_SIZE: usize = 1000;

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
        new_vertex: &Vertex,
    ) -> Result<Option<RepoPathBuf>>;

    /// Find the old path of the given new path in the old_tree
    async fn find_rename_backward(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
        new_path: &RepoPath,
        new_vertex: &Vertex,
    ) -> Result<Option<RepoPathBuf>>;
}

/// Rename finder based on the copy information in the file header metadata
pub struct MetadataRenameFinder {
    inner: RenameFinderInner,
}

/// Content similarity based Rename finder (mainly for Git repo)
pub struct ContentSimilarityRenameFinder {
    inner: RenameFinderInner,
}

/// RenameFinderInner is the base struct for MetadataRenameFinder and ContentSimilarityRenameFinder,
/// It is introduced for code reuse between those two file based rename finders.
struct RenameFinderInner {
    // Read content and rename metadata of a file
    file_reader: Arc<dyn FileStore>,
    // Read configs
    config: Arc<dyn Config + Send + Sync>,
    // Dir move caused rename candidates
    cache: Mutex<LruCache<CacheKey, Key>>,
}

type CacheKey = (Vertex, RepoPathBuf);

impl MetadataRenameFinder {
    pub fn new(
        file_reader: Arc<dyn FileStore>,
        config: Arc<dyn Config + Send + Sync>,
    ) -> Result<Self> {
        let cache_size = get_rename_cache_size(&config)?;
        let inner = RenameFinderInner {
            file_reader,
            config,
            cache: Mutex::new(LruCache::new(cache_size)),
        };
        Ok(Self { inner })
    }
}

#[async_trait]
impl RenameFinder for MetadataRenameFinder {
    async fn find_rename_forward(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
        old_path: &RepoPath,
        new_vertex: &Vertex,
    ) -> Result<Option<RepoPathBuf>> {
        let candidates = self.inner.generate_candidates(
            old_tree,
            new_tree,
            old_path,
            new_vertex,
            SearchDirection::Forward,
        )?;

        let found = self
            .inner
            .read_renamed_metadata_forward(candidates.clone(), old_path)
            .await?;

        if found.is_some() || !self.inner.get_fallback_to_content_similarity()? {
            return Ok(found);
        }

        // fallback to content similarity
        let old_path_key = self.inner.get_key_from_path(old_tree, old_path)?;
        let found = self
            .inner
            .find_similar_file(candidates, old_path_key)
            .await?;
        emit_content_similarity_fallback_metric(found.is_some());
        Ok(found)
    }

    async fn find_rename_backward(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
        new_path: &RepoPath,
        new_vertex: &Vertex,
    ) -> Result<Option<RepoPathBuf>> {
        let new_key = self.inner.get_key_from_path(new_tree, new_path)?;
        let found = self
            .inner
            .read_renamed_metadata_backward(new_key.clone())
            .await?;

        if found.is_some() || !self.inner.get_fallback_to_content_similarity()? {
            return Ok(found);
        }

        // fallback to content similarity
        let candidates = self.inner.generate_candidates(
            old_tree,
            new_tree,
            new_path,
            new_vertex,
            SearchDirection::Backward,
        )?;
        let found = self.inner.find_similar_file(candidates, new_key).await?;
        emit_content_similarity_fallback_metric(found.is_some());
        Ok(found)
    }
}

impl ContentSimilarityRenameFinder {
    pub fn new(
        file_reader: Arc<dyn FileStore>,
        config: Arc<dyn Config + Send + Sync>,
    ) -> Result<Self> {
        let cache_size = get_rename_cache_size(&config)?;
        let inner = RenameFinderInner {
            file_reader,
            config,
            cache: Mutex::new(LruCache::new(cache_size)),
        };
        Ok(Self { inner })
    }
}

#[async_trait]
impl RenameFinder for ContentSimilarityRenameFinder {
    async fn find_rename_forward(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
        old_path: &RepoPath,
        new_vertex: &Vertex,
    ) -> Result<Option<RepoPathBuf>> {
        self.inner
            .find_rename_in_direction(
                old_tree,
                new_tree,
                old_path,
                new_vertex,
                SearchDirection::Forward,
            )
            .await
    }

    async fn find_rename_backward(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
        new_path: &RepoPath,
        new_vertex: &Vertex,
    ) -> Result<Option<RepoPathBuf>> {
        self.inner
            .find_rename_in_direction(
                old_tree,
                new_tree,
                new_path,
                new_vertex,
                SearchDirection::Backward,
            )
            .await
    }
}

impl RenameFinderInner {
    fn generate_candidates(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
        path: &RepoPath,
        vertex: &Vertex,
        direction: SearchDirection,
    ) -> Result<Vec<Key>> {
        let cache_key = (vertex.clone(), path.to_owned());
        {
            let mut cache = self.cache.lock();
            if let Some(val) = cache.get_mut(&cache_key) {
                return Ok(vec![val.clone()]);
            }
        }

        let (mut added_files, mut deleted_files) =
            self.get_added_and_deleted_files(old_tree, new_tree)?;
        let batch_mv_candidates = detect_batch_move(&mut added_files, &mut deleted_files);

        if batch_mv_candidates.is_empty() {
            match direction {
                SearchDirection::Forward => {
                    select_rename_candidates(added_files, path, &self.config)
                }
                SearchDirection::Backward => {
                    select_rename_candidates(deleted_files, path, &self.config)
                }
            }
        } else {
            let mut candidates: Vec<Key> = vec![];
            let mut cache = self.cache.lock();

            match direction {
                SearchDirection::Forward => {
                    for (to, from) in batch_mv_candidates {
                        let key = (vertex.clone(), from.path);
                        cache.insert(key, to);
                    }
                }
                SearchDirection::Backward => {
                    for (to, from) in batch_mv_candidates {
                        let key = (vertex.clone(), to.path);
                        cache.insert(key, from);
                    }
                }
            }

            if let Some(val) = cache.get_mut(&cache_key) {
                candidates.push(val.to_owned());
            }
            Ok(candidates)
        }
    }

    fn get_added_and_deleted_files(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
    ) -> Result<(Vec<Key>, Vec<Key>)> {
        let mut added_files = Vec::new();
        let mut deleted_files = Vec::new();
        let matcher = AlwaysMatcher::new();
        let diff = Diff::new(old_tree, new_tree, &matcher)?;
        for entry in diff {
            let entry = entry?;
            match entry.diff_type {
                DiffType::RightOnly(file_metadata) => {
                    let path = entry.path;
                    let key = Key {
                        path,
                        hgid: file_metadata.hgid,
                    };
                    added_files.push(key);
                }
                DiffType::LeftOnly(file_metadata) => {
                    let path = entry.path;
                    let key = Key {
                        path,
                        hgid: file_metadata.hgid,
                    };
                    deleted_files.push(key);
                }
                _ => {}
            }
        }
        Ok((added_files, deleted_files))
    }

    async fn read_renamed_metadata_forward(
        &self,
        keys: Vec<Key>,
        old_path: &RepoPath,
    ) -> Result<Option<RepoPathBuf>> {
        tracing::trace!(keys_len = keys.len(), " read_renamed_metadata_forward");
        block_in_place(move || {
            let renames = self.file_reader.get_rename_iter(keys)?;
            for rename in renames {
                let (key, rename_from_key) = rename?;
                if rename_from_key.path.as_repo_path() == old_path {
                    return Ok(Some(key.path));
                }
            }
            Ok(None)
        })
    }

    async fn read_renamed_metadata_backward(&self, key: Key) -> Result<Option<RepoPathBuf>> {
        block_in_place(move || {
            let renames = self.file_reader.get_rename_iter(vec![key])?;
            for rename in renames {
                let (_, rename_from_key) = rename?;
                return Ok(Some(rename_from_key.path));
            }
            Ok(None)
        })
    }

    async fn find_rename_in_direction(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
        source_path: &RepoPath,
        new_vertex: &Vertex,
        direction: SearchDirection,
    ) -> Result<Option<RepoPathBuf>> {
        tracing::trace!(?source_path, ?direction, " find_rename_in_direction");

        let candidates =
            self.generate_candidates(old_tree, new_tree, source_path, new_vertex, direction)?;

        tracing::trace!(candidates_len = candidates.len(), " found");

        let source_tree = match direction {
            SearchDirection::Forward => old_tree,
            SearchDirection::Backward => new_tree,
        };
        let source = self.get_key_from_path(source_tree, source_path)?;

        self.find_similar_file(candidates, source).await
    }

    async fn find_similar_file(
        &self,
        keys: Vec<Key>,
        source_key: Key,
    ) -> Result<Option<RepoPathBuf>> {
        let source_content = spawn_blocking({
            let path = source_key.path.clone();
            let reader = self.file_reader.clone();
            move || reader.get_content(&path, source_key.hgid)
        })
        .await??;

        block_in_place(move || {
            let iter = self.file_reader.get_content_iter(keys)?;
            for entry in iter {
                let (k, candidate_content) = entry?;
                if is_content_similar(&source_content, &candidate_content, &self.config)? {
                    return Ok(Some(k.path));
                }
            }
            Ok(None)
        })
    }

    fn get_key_from_path(&self, tree: &TreeManifest, path: &RepoPath) -> Result<Key> {
        let key = match tree.get_file(path)? {
            None => return Err(CopyTraceError::FileNotFound(path.to_owned()).into()),
            Some(file_metadata) => Key {
                path: path.to_owned(),
                hgid: file_metadata.hgid,
            },
        };
        Ok(key)
    }

    fn get_fallback_to_content_similarity(&self) -> Result<bool> {
        let v = self
            .config
            .get_opt::<bool>("copytrace", "fallback-to-content-similarity")?
            .unwrap_or(DEFAULT_FALLBACK_TO_CONTENT_SIMILARITY);
        Ok(v)
    }
}

fn select_rename_candidates(
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
        let score = (file_path_similarity(path, source_path) * 1000.0) as i32;
        (-score, path.to_owned())
    });
    let max_rename_candidates = config
        .get_opt::<usize>("copytrace", "max-rename-candidates")?
        .unwrap_or(DEFAULT_MAX_RENAME_CANDIDATES);
    if candidates.len() > max_rename_candidates {
        Ok(candidates.into_iter().take(max_rename_candidates).collect())
    } else {
        Ok(candidates)
    }
}

fn detect_batch_move(added_files: &mut Vec<Key>, deleted_files: &mut Vec<Key>) -> Vec<(Key, Key)> {
    if added_files.len() != deleted_files.len() {
        return vec![];
    }

    added_files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    deleted_files.sort_unstable_by(|a, b| a.path.cmp(&b.path));

    let stripped_pairs: HashSet<(String, String)> = zip(added_files.clone(), deleted_files.clone())
        .map(|(a, b)| strip_common_prefix_and_suffix(a.path.as_str(), b.path.as_str()))
        .collect();

    if stripped_pairs.len() > 1 {
        vec![]
    } else {
        zip(added_files, deleted_files)
            .map(|(a, b)| (a.clone(), b.clone()))
            .collect()
    }
}

fn strip_common_prefix_and_suffix(s1: &str, s2: &str) -> (String, String) {
    let s1_chars: Vec<char> = s1.chars().collect();
    let s2_chars: Vec<char> = s2.chars().collect();

    let mut start = 0;
    while start < s1_chars.len() && start < s2_chars.len() && s1_chars[start] == s2_chars[start] {
        start += 1;
    }

    let mut end = 0;
    while end < s1_chars.len() - start
        && end < s2_chars.len() - start
        && s1_chars[s1_chars.len() - 1 - end] == s2_chars[s2_chars.len() - 1 - end]
    {
        end += 1;
    }

    let stripped_s1: String = s1_chars[start..s1_chars.len() - end].iter().collect();
    let stripped_s2: String = s2_chars[start..s2_chars.len() - end].iter().collect();
    (stripped_s1, stripped_s2)
}

fn get_rename_cache_size(config: &dyn Config) -> Result<usize> {
    let v = config
        .get_opt::<usize>("copytrace", "rename-cache-size")?
        .unwrap_or(DEFAULT_RENAME_CACHE_SIZE);
    Ok(v)
}

fn emit_content_similarity_fallback_metric(is_found: bool) {
    let metric = if is_found {
        "copytrace_content_similarity_fallback_success"
    } else {
        "copytrace_content_similarity_fallback_failure"
    };
    increment_counter(metric, 1);
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

    #[test]
    fn test_detect_batch_move() {
        let mut added_files: Vec<Key> = vec![
            gen_key("a/b/c.txt"),
            gen_key("a/b/c.md"),
            gen_key("a/d.txt"),
        ];

        let mut deleted_files: Vec<Key> = vec![
            gen_key("b/b/c.txt"),
            gen_key("b/b/c.md"),
            gen_key("b/d.txt"),
        ];

        assert_eq!(
            detect_batch_move(&mut added_files, &mut deleted_files),
            vec![
                (gen_key("a/b/c.md"), gen_key("b/b/c.md")),
                (gen_key("a/b/c.txt"), gen_key("b/b/c.txt")),
                (gen_key("a/d.txt"), gen_key("b/d.txt")),
            ]
        );
    }

    #[test]
    fn test_detect_batch_move_with_unequal_num_of_files() {
        let mut added_files: Vec<Key> = vec![
            gen_key("a/b/c.txt"),
            gen_key("a/b/c.md"),
            gen_key("a/d.txt"),
            gen_key("a/e.txt"),
        ];

        let mut deleted_files: Vec<Key> = vec![
            gen_key("b/b/c.txt"),
            gen_key("b/b/c.md"),
            gen_key("b/d.txt"),
        ];

        assert_eq!(
            detect_batch_move(&mut added_files, &mut deleted_files),
            vec![]
        );
    }

    #[test]
    fn test_detect_batch_move_with_unmatched_basename() {
        let mut added_files: Vec<Key> = vec![
            gen_key("a/b/c.txt"),
            gen_key("a/b/c.md"),
            gen_key("a/d.txt"),
        ];

        let mut deleted_files: Vec<Key> = vec![
            gen_key("b/b/ccc.txt"),
            gen_key("b/b/c.md"),
            gen_key("b/d.txt"),
        ];

        assert_eq!(
            detect_batch_move(&mut added_files, &mut deleted_files),
            vec![]
        );
    }

    #[test]
    fn test_strip_common_prefix_and_suffix() {
        let func = strip_common_prefix_and_suffix;

        assert_eq!(func("", ""), ("".to_owned(), "".to_owned()));
        assert_eq!(func("", "a"), ("".to_owned(), "a".to_owned()));
        assert_eq!(func("a", ""), ("a".to_owned(), "".to_owned()));

        assert_eq!(func("1a2", "1b2"), ("a".to_owned(), "b".to_owned()));
        assert_eq!(func("1a22", "1b2"), ("a2".to_owned(), "b".to_owned()));

        assert_eq!(
            func("/a/1.txt", "/a/1.md"),
            ("txt".to_owned(), "md".to_owned())
        );
        assert_eq!(
            func("/a/b/1.txt", "/a/c/1.txt"),
            ("b".to_owned(), "c".to_owned())
        );

        assert_eq!(
            func("/文件/我的/好.txt", "/文件/你的/好.txt"),
            ("我".to_owned(), "你".to_owned()),
        );
    }
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Testing.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::bail;
use anyhow::Result;
use async_trait::async_trait;
use dag::ops::DagAddHeads;
use dag::ops::DagAlgorithm;
use dag::MemDag;
use dag::NameSet;
use dag::Vertex;
use manifest::FileMetadata;
use manifest::FileType;
use manifest::Manifest;
use manifest_tree::TreeManifest;
use sha1::Digest;
use sha1::Sha1;
use storemodel::minibytes::Bytes;
use storemodel::InsertOpts;
use storemodel::KeyStore;
use storemodel::ReadRootTreeIds;
use storemodel::SerializationFormat;
use storemodel::TreeStore;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::RepoPathBuf;

use crate::PathHistory;
use crate::RenameTracer;

#[derive(Clone, Default)]
pub struct TestHistory {
    inner: Arc<Mutex<TestHistoryInner>>,
}

#[derive(Default)]
struct TestHistoryInner {
    /// Commits that change trees.
    commit_to_tree: BTreeMap<u64, HgId>,
    /// Tree store in git format.
    trees: BTreeMap<HgId, Bytes>,
    /// Override commit parents. By default commit x has parent [x-1], commit 0 has no parents.
    commit_parents: BTreeMap<u64, Vec<u64>>,
    /// Empty tree id.
    empty_tree_id: HgId,
    /// Prefetched trees.
    prefetched_trees: HashSet<Key>,
    /// Prefetch logs.
    access_log: Vec<String>,
}

impl TestHistory {
    /// Construct history with changes in this format:
    /// `(commit_index, path, file_content_int, file_type)`.
    /// `commit_index` is an integer that will be translated to a commit hash.
    /// By default, the commit graph is fully linear and 0 is the root commit.
    /// `file_content_int` decides a fake content hash. 0 is special and means deletion.
    ///
    /// Changes are applied in commit order, and are accumulated.
    pub fn from_history(commit_path_content: &[(u64, &'static str, u64, FileType)]) -> Self {
        let this = Self::default();
        {
            let inner = this.inner.lock().unwrap();
            assert!(
                inner.commit_to_tree.is_empty(),
                "history can only be inserted once"
            );
        }

        let mut input = commit_path_content.to_vec();
        input.sort();

        let mut last_commit_int = 0;
        let mut tree = TreeManifest::ephemeral(Arc::new(this.clone()));
        // Write empty tree.
        let empty_tree_id = tree.flush().unwrap();
        for (commit_int, path, content_int, file_type) in input {
            if commit_int > last_commit_int {
                // Commit last_commit.
                let tree_id = tree.flush().unwrap();
                let mut inner = this.inner.lock().unwrap();
                inner.commit_to_tree.insert(last_commit_int, tree_id);
                last_commit_int = commit_int;
            }
            let path = RepoPath::from_str(path).unwrap().to_owned();
            if content_int == 0 {
                tree.remove(&path).unwrap();
            } else {
                let meta = FileMetadata {
                    hgid: hgid_from_int(content_int),
                    file_type,
                };
                tree.insert(path, meta).unwrap();
            }
        }

        let tree_id = tree.flush().unwrap();
        {
            let mut inner = this.inner.lock().unwrap();
            inner.commit_to_tree.insert(last_commit_int, tree_id);
            inner.empty_tree_id = empty_tree_id;
        }

        this
    }

    /// Override parents. By default, the graph is fully linear. This allows
    /// creating non-linear graphs.
    pub fn set_commit_parents(&self, commit_id: u64, parent_ids: &[u64]) -> &Self {
        let mut inner = self.inner.lock().unwrap();
        inner.commit_parents.insert(commit_id, parent_ids.to_vec());
        self
    }

    /// Obtain the `PathHistory` struct.
    pub async fn paths_history(
        &self,
        params: impl Into<BuildSetParam>,
        paths: &[&str],
    ) -> PathHistory {
        // Build commit graph and the "set".
        let set = self.build_set(params.into()).await;

        // Convert path types.
        let paths: Vec<RepoPathBuf> = paths
            .iter()
            .map(|s| RepoPath::from_str(s).unwrap().to_owned())
            .collect();

        PathHistory::new(set, paths, Arc::new(self.clone()), Arc::new(self.clone()))
            .await
            .unwrap()
    }

    /// Obtain the access log.
    pub fn take_access_log(&self) -> Vec<String> {
        self.inner.lock().unwrap().access_log.drain(..).collect()
    }

    fn commit_to_tree(&self, commit_id: HgId) -> HgId {
        let commit_int = hgid_to_int(commit_id);
        let inner = self.inner.lock().unwrap();
        match inner.commit_to_tree.range(..=commit_int).next_back() {
            Some((_, tree_id)) => *tree_id,
            None => inner.empty_tree_id,
        }
    }

    async fn build_set(&self, build_set_params: BuildSetParam) -> NameSet {
        let BuildSetParam(max_commit_int, selected_commit_ints) = build_set_params;
        // Build commit graph and the "set".
        let mut dag = MemDag::new();
        let parents_override = {
            let inner = self.inner.lock().unwrap();
            inner.commit_parents.clone()
        };
        let parents = move |v: Vertex| -> dag::Result<Vec<Vertex>> {
            let i = vertex_to_int(v);
            let mut ps = Vec::new();
            match parents_override.get(&i) {
                None => {
                    if i > 0 {
                        ps.push(vertex_from_int(i - 1));
                    }
                }
                Some(ids) => {
                    for &p in ids {
                        ps.push(vertex_from_int(p));
                    }
                }
            }
            Ok(ps)
        };
        let parents: Box<dyn Fn(Vertex) -> dag::Result<Vec<Vertex>> + Send + Sync> =
            Box::new(parents);
        for i in 0..=max_commit_int {
            let head = vertex_from_int(i);
            let heads = vec![head.clone()];
            dag.add_heads(&parents, &heads.into()).await.unwrap();
        }

        match selected_commit_ints {
            None => dag.all().await.unwrap(),
            Some(ints) => {
                let set = NameSet::from_static_names(ints.into_iter().map(vertex_from_int));
                dag.sort(&set).await.unwrap()
            }
        }
    }

    // Obtain the `RenameTracer` struct.
    pub async fn rename_tracer(
        &self,
        params: impl Into<BuildSetParam>,
        path: &str,
    ) -> RenameTracer {
        let set = self.build_set(params.into()).await;
        let path = RepoPath::from_str(path).unwrap().to_owned();
        RenameTracer::new(set, path, Arc::new(self.clone()), Arc::new(self.clone()))
            .await
            .unwrap()
    }
}

/// Used by `build_set`. Specifies the graph size and the subset passed to PathHistory.
pub struct BuildSetParam(u64, Option<Vec<u64>>);

impl From<u64> for BuildSetParam {
    fn from(value: u64) -> Self {
        Self(value, None)
    }
}

impl<I: Iterator<Item = u64>> From<(u64, I)> for BuildSetParam {
    fn from(value: (u64, I)) -> Self {
        Self(value.0, Some(value.1.collect()))
    }
}

fn compute_sha1(content: &[u8]) -> HgId {
    let mut hasher = Sha1::new();
    hasher.update(format!("tree {}\0", content.len()));
    hasher.update(content);
    let buf: [u8; HgId::len()] = hasher.finalize().into();
    (&buf).into()
}

#[async_trait]
impl KeyStore for TestHistory {
    fn get_local_content(&self, path: &RepoPath, hgid: HgId) -> anyhow::Result<Option<Bytes>> {
        let key = Key::new(path.to_owned(), hgid);
        let inner = self.inner.lock().unwrap();
        if !inner.prefetched_trees.contains(&key) {
            bail!("not prefetched: {:?}", &key);
        }
        match inner.trees.get(&hgid) {
            Some(v) => Ok(Some(v.clone())),
            None => bail!("{:?} not found", &key),
        }
    }

    fn insert_data(
        &self,
        _opts: InsertOpts,
        _path: &RepoPath,
        data: &[u8],
    ) -> anyhow::Result<HgId> {
        let hgid = compute_sha1(data);
        self.inner
            .lock()
            .unwrap()
            .trees
            .insert(hgid, Bytes::copy_from_slice(data));
        Ok(hgid)
    }

    fn prefetch(&self, mut keys: Vec<Key>) -> Result<()> {
        keys.sort();
        let log = keys
            .iter()
            .map(|k| format!("{}/{}", &k.hgid.to_hex()[..5], k.path.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        let log = format!("Trees: [{}]", log);
        let mut inner = self.inner.lock().unwrap();
        for key in keys {
            inner.prefetched_trees.insert(key);
        }
        inner.access_log.push(log);
        Ok(())
    }

    fn format(&self) -> SerializationFormat {
        SerializationFormat::Git
    }
}

impl TreeStore for TestHistory {}

#[async_trait]
impl ReadRootTreeIds for TestHistory {
    async fn read_root_tree_ids(&self, commits: Vec<HgId>) -> anyhow::Result<Vec<(HgId, HgId)>> {
        let log = commits
            .iter()
            .map(|id| hgid_to_int(*id).to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let log = format!("Commits: [{}]", log);
        let result = commits
            .into_iter()
            .map(|commit_id| {
                let tree_id = self.commit_to_tree(commit_id);
                (commit_id, tree_id)
            })
            .collect();
        self.inner.lock().unwrap().access_log.push(log);
        Ok(result)
    }
}

fn hgid_from_int(v: u64) -> HgId {
    let mut bytes = v.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[0; HgId::len() - 8]);
    HgId::from_slice(&bytes).unwrap()
}

fn hgid_to_int(id: HgId) -> u64 {
    let bytes = &id.as_ref()[0..8];
    let bytes: [u8; 8] = bytes.try_into().unwrap();
    u64::from_le_bytes(bytes)
}

fn vertex_from_int(v: u64) -> Vertex {
    let id = hgid_from_int(v);
    Vertex::copy_from(id.as_ref())
}

fn vertex_to_int(v: Vertex) -> u64 {
    let id = HgId::from_slice(v.as_ref()).unwrap();
    hgid_to_int(id)
}

impl PathHistory {
    async fn next_n(&mut self, mut n: usize) -> Vec<u64> {
        let mut result = Vec::new();
        while let Some(v) = self.next().await.unwrap() {
            result.push(vertex_to_int(v));
            n -= 1;
            if n == 0 {
                break;
            }
        }
        result
    }
}

impl RenameTracer {
    async fn next_n(&mut self, mut n: usize) -> Vec<u64> {
        let mut result = Vec::new();
        while let Some(v) = self.next().await.unwrap() {
            result.push(vertex_to_int(v));
            n -= 1;
            if n == 0 {
                break;
            }
        }
        result
    }
}

// Tests

use FileType::Executable as E;
use FileType::Regular as R;
use FileType::Symlink as S;

#[tokio::test]
async fn test_log_files() {
    let t = TestHistory::from_history(&[
        (0, "a", 1, R),
        (100, "a", 0, R),
        (150, "b", 4, R),
        (200, "a", 3, E),
        (250, "b", 5, E),
    ]);

    let mut h = t.paths_history(300, &["a"]).await;
    assert_eq!(h.next_n(3).await, [200, 100, 0]);

    let _ = t.take_access_log();
    let mut h = t.paths_history(199, &["a"]).await;
    assert_eq!(h.next_n(1).await, [100]);
    assert_eq!(
        t.take_access_log(),
        [
            "Commits: [15, 31, 47, 63, 79, 95, 111, 127, 143, 159, 175, 191, 192, 193, 194, 195, 196, 197, 198, 199, 0]",
            "Trees: [0841a/, 4b825/, 7ba4c/]",
            "Commits: [96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110]",
            "Trees: [0841a/, 4b825/]"
        ]
    );

    let mut h = t.paths_history(250, &["b"]).await;
    assert_eq!(h.next_n(3).await, [250, 150]);

    let mut h = t.paths_history(300, &["a", "b"]).await;
    assert_eq!(h.next_n(10).await, [250, 200, 150, 100, 0]);
}

#[tokio::test]
async fn test_log_dirs() {
    let t = TestHistory::from_history(&[
        (0, "a/b/c/d", 1, R),
        (100, "a/b/c/e", 2, R),
        (150, "a/b/c/d", 0, R),
        (150, "a/b/c/e", 0, R),
        (200, "h/i/j/k", 5, E),
        (250, "a/b/c/f", 3, R),
    ]);

    let mut h = t.paths_history(300, &["a"]).await;
    assert_eq!(h.next_n(5).await, [250, 150, 100, 0]);

    let mut h = t.paths_history(300, &["a/b"]).await;
    assert_eq!(h.next_n(5).await, [250, 150, 100, 0]);

    let mut h = t.paths_history(300, &["a/b/c"]).await;
    assert_eq!(h.next_n(5).await, [250, 150, 100, 0]);

    // answers "who deletes" query too
    let mut h = t.paths_history(300, &["a/b/c/d"]).await;
    assert_eq!(h.next_n(5).await, [150, 0]);

    let mut h = t.paths_history(300, &["a/b/c/d", "h"]).await;
    assert_eq!(h.next_n(5).await, [200, 150, 0]);

    // Check tree prefetch behavior.
    let _ = t.take_access_log();
    let mut h = t.paths_history(300, &["a/b", "h/i"]).await;
    assert_eq!(h.next_n(5).await, [250, 200, 150, 100, 0]);
    assert_eq!(
        t.take_access_log(),
        [
            "Commits: [255, 271, 287, 288, 289, 290, 291, 292, 293, 294, 295, 296, 297, 298, 299, 300, 0]",
            "Trees: [21f04/, d8714/]",
            "Trees: [10831/a, 6f98e/a, 4429c/h]",
            "Commits: [15, 31, 47, 63, 79, 95, 111, 127, 143, 159, 175, 191, 207, 223, 239]",
            "Trees: [21f04/, 4b825/, 991a1/, e25ad/]",
            "Trees: [10831/a, 8b8c5/a, 4429c/h]",
            "Commits: [240, 241, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252, 253, 254]",
            "Trees: [991a1/, d8714/]",
            "Trees: [6f98e/a, 4429c/h]",
            "Commits: [192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 202, 203, 204, 205, 206]",
            "Trees: [4b825/, 991a1/]",
            "Trees: [4429c/h]",
            "Commits: [144, 145, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 157, 158]",
            "Trees: [4b825/, e25ad/]",
            "Trees: [8b8c5/a]",
            "Commits: [96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110]",
            "Trees: [21f04/, e25ad/]",
            "Trees: [10831/a, 8b8c5/a]",
            "Commits: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]",
            "Trees: [21f04/]",
            "Trees: [10831/a]"
        ]
    );
}

#[tokio::test]
async fn test_log_with_roots() {
    // Use a commit graph with a few roots.
    let t = TestHistory::from_history(&[(0, "a", 1, R)]);
    t.set_commit_parents(10, &[]);
    t.set_commit_parents(11, &[10, 9]);
    t.set_commit_parents(90, &[]);
    t.set_commit_parents(121, &[89, 120]);

    // Roots are outputted, despite there are no changes.
    let mut h = t.paths_history(300, &["a"]).await;
    assert_eq!(h.next_n(5).await, [90, 10, 0]);

    // Roots are not outputted if the path does not exist.
    let mut h = t.paths_history(300, &["b"]).await;
    assert_eq!(h.next_n(5).await, &[] as &[u64]);
}

#[tokio::test]
async fn test_log_merge_same_with_parent() {
    // b--------merge
    //     /
    // a---
    let n = 8;
    for index_a in 1..n {
        for index_b in index_a + 1..n {
            for index_merge in index_b + 1..n {
                for merge_content in [1, 2] {
                    let t = TestHistory::from_history(&[
                        (0, "a", 0, R),
                        (index_b, "a", 1, R),
                        (index_a, "a", 2, R),
                        (index_merge, "a", merge_content, R),
                    ]);
                    t.set_commit_parents(index_a, &[]);
                    t.set_commit_parents(index_b, &[]);
                    t.set_commit_parents(index_merge, &[index_b, index_a]);

                    // Log should not contain the merge.
                    // It is the same as one parent.
                    let mut h = t.paths_history(n, &["a"]).await;
                    assert!(!h.next_n(5).await.contains(&index_merge));

                    // Swap parents. No merge in history too.
                    t.set_commit_parents(index_merge, &[index_a, index_b]);
                    let mut h = t.paths_history(n, &["a"]).await;
                    assert!(!h.next_n(5).await.contains(&index_merge));
                }
            }
        }
    }
}

#[tokio::test]
async fn test_log_muti_heads_in_testing_range() {
    // This test targets the plain "bisect" algorithm that was
    // tried before using segments. It is generally useful
    // for testing correctness.
    //
    // In revset syntax, (heads(TESTING) - right) is non-empty.
    // Some changes might "escape" the TESTING range check.
    //
    // ---low---escape-----------merge---
    //      \                      /
    //       -------------high-----
    //
    // Index-wise, low (right) < escape < high (left).
    // Content-wise, left and right are same on paths.
    // merge contains "escaped" changes from "escape".

    // Range:
    // Step 1: 1------------------------------2
    // Step 2: 1-------------1                2
    // Step 3:               1---3----1
    //         ^      ^      ^   ^    ^       ^
    //         128   96     64   48   32      0
    //             merge  left escape right
    // Graph:  v      v      v   v    v       v
    //         1  ----3------1--------1-------2
    //                 \             /
    //                  ---------3---
    //
    // Run with `LOG=pathhistory=trace` to see trace.
    let t = TestHistory::from_history(&[
        (0, "a", 2, R),
        (32, "a", 1, R), // right (low)
        (48, "a", 3, R), // escape
        (64, "a", 1, R), // left (high)
        (96, "a", 3, R), // merge
        (120, "a", 1, R),
    ]);
    t.set_commit_parents(64, &[32]);
    t.set_commit_parents(96, &[95, 63]);
    t.set_commit_parents(120, &[]);

    // Log should contain the "escape" commit (48).
    let mut h = t.paths_history(128, &["a"]).await;
    assert_eq!(h.next_n(9).await, [120, 48, 32, 0]);
}

#[tokio::test]
async fn test_log_subset_misleading_parents() {
    // a: 0..80 (0), 80..90 (1), 90..100 (0)
    let t = TestHistory::from_history(&[
        (79, "a", 0, R),
        (80, "a", 1, R),
        (90, "a", 0, R),
        (100, "a", 0, R),
    ]);

    // When listing all changes, PathHistory will miss the change, because "a" is consistently
    // missing even if pathhistory bisects once - it still decide to skip the 50-100 range.
    let mut h = t.paths_history((100, 0..=100), &["a"]).await;
    assert_eq!(h.next_n(9).await, &[] as &[u64]);

    // When we pass a subset that is not "ancestors(something)" to PathHistory, it can detect
    // changes in the given range.
    let mut h = t.paths_history((100, 85..=95), &["a"]).await;
    assert_eq!(h.next_n(9).await, &[90]);

    // In this case, we expect PathHistory to not skip the 80 to 95 range by detecting "a" is the
    // same in 79 (parent of 80) and 95.
    let mut h = t.paths_history((100, 80..=95), &["a"]).await;
    assert_eq!(h.next_n(9).await, &[90, 80]);
}

#[tokio::test]
async fn test_log_with_mode_only_changes() {
    let t = TestHistory::from_history(&[(0, "a", 1, R), (100, "a", 1, E), (200, "a", 1, S)]);
    let mut h = t.paths_history(300, &["a"]).await;
    assert_eq!(h.next_n(5).await, [200, 100, 0]);
}

#[cfg(test)]
mod rename_tracer_tests {
    use super::*;

    #[tokio::test]
    async fn test_trace_rename_files() {
        let t = TestHistory::from_history(&[
            (0, "a", 1, R),
            (100, "a", 2, R),
            // rename a -> b
            (150, "a", 0, R),
            (150, "b", 4, R),
            (200, "b", 4, R),
            (250, "b", 5, E),
        ]);

        let mut r = t.rename_tracer(250, "b").await;
        assert_eq!(r.next_n(2).await, [150]);

        // commit 150 did the rename (b -> a), so we search commit 149 (150-1)
        // with name "a"
        let mut r = t.rename_tracer(149, "a").await;
        assert_eq!(r.next_n(2).await, [0]);
    }

    #[tokio::test]
    async fn test_trace_nested_rename_files() {
        let t = TestHistory::from_history(&[
            (0, "a/b/1", 1, R),
            (100, "a/b/1", 2, R),
            // rename a/b/1 -> a/2
            (150, "a/b/1", 0, R),
            (150, "a/2", 4, R),
            (200, "a/2", 4, R),
            (250, "a/2", 5, E),
        ]);

        let mut r = t.rename_tracer(250, "a/2").await;
        assert_eq!(r.next_n(2).await, [150]);

        // commit 150 did the rename (b -> a), so we search commit 149 (150-1)
        // with name "a"
        let mut r = t.rename_tracer(149, "a/b/1").await;
        assert_eq!(r.next_n(2).await, [0]);
    }

    #[tokio::test]
    async fn test_trace_commit_added_files() {
        let t = TestHistory::from_history(&[
            (0, "a", 1, R),
            (100, "a", 2, R),
            (150, "b", 3, R),
            (160, "a", 4, R),
            (200, "b", 5, R),
            (210, "b", 6, R),
            (220, "b", 7, R),
            (250, "b", 7, E),
        ]);

        let mut r = t.rename_tracer(250, "a").await;
        assert_eq!(r.next_n(2).await, [0]);

        let mut r = t.rename_tracer(250, "b").await;
        assert_eq!(r.next_n(2).await, [150]);
    }
}

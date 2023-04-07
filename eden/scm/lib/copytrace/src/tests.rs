/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Testing.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dag::ops::ImportAscii;
use dag::DagAlgorithm;
use dag::MemDag;
use dag::Vertex;
use manifest::FileMetadata;
use manifest::FileType;
use manifest::Manifest;
use manifest_tree::testutil::TestStore;
use manifest_tree::TreeManifest;
use manifest_tree::TreeStore;
use storemodel::futures::stream;
use storemodel::futures::StreamExt;
use storemodel::ReadFileContents;
use storemodel::ReadRootTreeIds;
use storemodel::TreeFormat;
use tracing_test::traced_test;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::RepoPathBuf;

use crate::CopyTrace;
use crate::DagCopyTrace;

#[derive(Clone)]
struct CopyTraceTestCase {
    inner: Arc<CopyTraceTestCaseInner>,
}

struct CopyTraceTestCaseInner {
    /// Commits that change trees.
    commit_to_tree: HashMap<Vertex, HgId>,
    /// In memory tree store
    tree_store: Arc<dyn TreeStore + Send + Sync>,
    /// dag algorithm
    dagalgo: Arc<dyn DagAlgorithm + Send + Sync>,
    /// commit renames
    renames: HashMap<Key, Key>,
}

#[derive(Debug)]
enum Change {
    Add(RepoPathBuf, FileMetadata),
    Delete(RepoPathBuf),
    Rename(RepoPathBuf, RepoPathBuf),
    Modify(RepoPathBuf, FileMetadata),
}

impl CopyTraceTestCase {
    pub async fn new(text: &str, changes: HashMap<&str, Vec<&str>>) -> Self {
        let mut mem_dag = MemDag::new();
        mem_dag
            .import_ascii_with_vertex_fn(text, vertex_from_str)
            .unwrap();

        let mut commit_to_tree: HashMap<Vertex, HgId> = Default::default();
        let mut renames: HashMap<Key, Key> = Default::default();
        let tree_store = Arc::new(TestStore::new().with_format(TreeFormat::Git));
        let changes = Change::build_changes(changes);

        // iterate through the commit graph to build trees in topo order (ascending)
        let set = mem_dag.all().await.unwrap();
        let mut iter = set.iter_rev().await.unwrap();
        while let Some(item) = iter.next().await {
            Self::build_tree(
                &mem_dag,
                tree_store.clone(),
                &mut commit_to_tree,
                &mut renames,
                item.unwrap(),
                &changes,
            )
            .await;
        }

        let inner = Arc::new(CopyTraceTestCaseInner {
            commit_to_tree,
            tree_store,
            dagalgo: Arc::new(mem_dag),
            renames,
        });
        CopyTraceTestCase { inner }
    }

    /// Create a DagCopyTrace instance
    pub async fn copy_trace(&self) -> Arc<dyn CopyTrace + Send + Sync> {
        let root_tree_reader = Arc::new(self.clone());
        let file_reader = Arc::new(self.clone());
        let tree_store = self.inner.tree_store.clone();
        let dagalgo = self.inner.dagalgo.clone();
        let copy_trace =
            DagCopyTrace::new(root_tree_reader, tree_store, file_reader, dagalgo).unwrap();
        Arc::new(copy_trace)
    }

    async fn build_tree(
        dag: &MemDag,
        tree_store: Arc<dyn TreeStore + Send + Sync>,
        commit_to_tree: &mut HashMap<Vertex, HgId>,
        renames: &mut HashMap<Key, Key>,
        commit: Vertex,
        changes: &HashMap<Vertex, Vec<Change>>,
    ) {
        let parents = dag.parent_names(commit.clone()).await.unwrap();
        let mut tree = match parents.first() {
            None => TreeManifest::ephemeral(tree_store.clone()),
            Some(p1) => {
                let tree_id = commit_to_tree[p1].clone();
                TreeManifest::durable(tree_store.clone(), tree_id)
            }
        };
        if let Some(val) = changes.get(&commit) {
            for v in val {
                match v {
                    Change::Add(path, metadata) => tree.insert(path.clone(), *metadata).unwrap(),
                    Change::Delete(path) => {
                        tree.remove(path).unwrap();
                    }
                    Change::Modify(path, metadata) => tree.insert(path.clone(), *metadata).unwrap(),
                    Change::Rename(from_path, to_path) => {
                        let file_metadata = tree.get_file(from_path).unwrap().unwrap();
                        tree.remove(from_path).unwrap();
                        tree.insert(to_path.clone(), file_metadata).unwrap();

                        // update renames mapping
                        let to_key = Key::new(to_path.clone(), file_metadata.hgid);
                        let from_key = Key::new(from_path.clone(), file_metadata.hgid);
                        renames.insert(to_key, from_key);
                    }
                }
            }
        }

        let tree_id = tree.flush().unwrap();
        commit_to_tree.insert(commit, tree_id);
    }
}

#[async_trait]
impl ReadRootTreeIds for CopyTraceTestCase {
    async fn read_root_tree_ids(&self, commits: Vec<HgId>) -> Result<Vec<(HgId, HgId)>> {
        let result = commits
            .into_iter()
            .map(|commit_id| {
                let commit = Vertex::copy_from(commit_id.as_ref());
                let tree_id = self.inner.commit_to_tree.get(&commit).unwrap().clone();
                (commit_id, tree_id)
            })
            .collect();
        Ok(result)
    }
}

#[async_trait]
impl ReadFileContents for CopyTraceTestCase {
    type Error = anyhow::Error;

    #[allow(dead_code)]
    async fn read_file_contents(
        &self,
        _keys: Vec<Key>,
    ) -> stream::BoxStream<Result<(storemodel::minibytes::Bytes, Key), Self::Error>> {
        // We will need this for computing content similarity score later.
        todo!()
    }

    async fn read_rename_metadata(
        &self,
        keys: Vec<Key>,
    ) -> stream::BoxStream<Result<(Key, Option<Key>), Self::Error>> {
        let renames: Vec<_> = {
            keys.iter()
                .map(|k| Ok((k.clone(), self.inner.renames.get(k).cloned())))
                .collect()
        };
        stream::iter(renames).boxed()
    }
}

impl Change {
    pub(crate) fn from_str(s: &str) -> Self {
        let items: Vec<_> = s.split(' ').collect();
        let change = match items[0] {
            "+" => Change::Add(to_repo_path_buf(items[1]), to_file_metadata(items[2])),
            "-" => Change::Delete(to_repo_path_buf(items[1])),
            "M" => Change::Modify(to_repo_path_buf(items[1]), to_file_metadata(items[2])),
            "->" => Change::Rename(to_repo_path_buf(items[1]), to_repo_path_buf(items[2])),
            _ => unreachable!("unexpected token {}", items[0]),
        };
        return change;

        fn to_repo_path_buf(s: &str) -> RepoPathBuf {
            RepoPath::from_str(s).unwrap().to_owned()
        }

        fn to_file_metadata(s: &str) -> FileMetadata {
            FileMetadata::new(hgid_from_str(s), FileType::Regular)
        }
    }

    pub(crate) fn build_changes(changes: HashMap<&str, Vec<&str>>) -> HashMap<Vertex, Vec<Change>> {
        changes
            .into_iter()
            .map(|(k, vs)| {
                (
                    vertex_from_str(k),
                    vs.iter().map(|v| Change::from_str(v)).collect(),
                )
            })
            .collect()
    }
}

fn hgid_from_str(s: &str) -> HgId {
    let mut bytes = s.as_bytes().to_vec();
    bytes.resize(HgId::len(), 0);
    HgId::from_slice(&bytes).unwrap()
}

fn vertex_from_str(s: &str) -> Vertex {
    let mut bytes = s.as_bytes().to_vec();
    bytes.resize(HgId::len(), 0);
    Vertex::copy_from(&bytes)
}

async fn trace_rename(
    c: &Arc<dyn CopyTrace + Send + Sync>,
    src: &str,
    dst: &str,
    src_path: &str,
) -> Result<Option<RepoPathBuf>> {
    let src = vertex_from_str(src);
    let dst = vertex_from_str(dst);
    let src_path = RepoPath::from_str(src_path).unwrap().to_owned();
    c.trace_rename(src, dst, src_path).await
}

fn p(path: &str) -> Option<RepoPathBuf> {
    Some(RepoPath::from_str(path).unwrap().to_owned())
}

#[tokio::test]
async fn test_linear_single_rename() {
    let ascii = r#"
    C
    |
    B
    |
    A
    "#;
    let changes = HashMap::from([
        ("A", vec!["+ a 1", "+ c 2"]),
        ("B", vec!["-> a b"]),
        ("C", vec!["M b 3"]),
    ]);
    let t = CopyTraceTestCase::new(ascii, changes).await;
    let c = t.copy_trace().await;

    let res = trace_rename(&c, "A", "C", "a").await.unwrap();
    assert_eq!(res, p("b"));
    let res = trace_rename(&c, "C", "A", "b").await.unwrap();
    assert_eq!(res, p("a"));

    let res = trace_rename(&c, "A", "B", "a").await.unwrap();
    assert_eq!(res, p("b"));
    let res = trace_rename(&c, "B", "A", "b").await.unwrap();
    assert_eq!(res, p("a"));

    let res = trace_rename(&c, "A", "C", "d").await.unwrap();
    assert_eq!(res, None);
}

#[tokio::test]
#[traced_test]
async fn test_non_linear_single_rename() {
    let ascii = r#"
    B C
    |/
    A
    |
    S
    "#;
    let changes = HashMap::from([
        ("S", vec!["+ s 0"]),
        ("A", vec!["+ a 1", "+ c 2"]),
        ("B", vec!["-> a b"]),
        ("C", vec!["M a 3"]),
    ]);
    let t = CopyTraceTestCase::new(ascii, changes).await;
    let c = t.copy_trace().await;

    let res = trace_rename(&c, "C", "B", "a").await.unwrap();
    assert_eq!(res, p("b"));
    let res = trace_rename(&c, "B", "C", "b").await.unwrap();
    assert_eq!(res, p("a"));
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Testing.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dag::DagAlgorithm;
use dag::MemDag;
use dag::Vertex;
use dag::ops::ImportAscii;
use manifest::FileMetadata;
use manifest::FileType;
use manifest::Manifest;
use manifest_tree::TreeManifest;
use manifest_tree::TreeStore;
use manifest_tree::testutil::TestStore;
use storemodel::BoxIterator;
use storemodel::FileStore;
use storemodel::KeyStore;
use storemodel::ReadRootTreeIds;
use storemodel::SerializationFormat;
use storemodel::futures::StreamExt;
use tracing_test::traced_test;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::RepoPathBuf;

use crate::CopyTrace;
use crate::DagCopyTrace;
use crate::MetadataRenameFinder;
use crate::TraceResult;

#[derive(Clone)]
struct CopyTraceTestCase {
    inner: Arc<CopyTraceTestCaseInner>,
}

struct CopyTraceTestCaseInner {
    /// Commits that change trees.
    commit_to_tree: HashMap<Vertex, HgId>,
    /// In memory tree store
    tree_store: Arc<dyn TreeStore>,
    /// Dag algorithm
    dagalgo: Arc<dyn DagAlgorithm + Send + Sync>,
    /// Copies info: dest -> src mapping
    copies: HashMap<Key, Key>,
    /// Config
    config: BTreeMap<&'static str, &'static str>,
}

#[derive(Debug)]
enum Change {
    Add(RepoPathBuf, FileMetadata),
    Delete(RepoPathBuf),
    Rename(RepoPathBuf, RepoPathBuf),
    Modify(RepoPathBuf, FileMetadata),
    Copy(RepoPathBuf, RepoPathBuf),
}

impl CopyTraceTestCase {
    pub async fn new(text: &str, changes: HashMap<&str, Vec<&str>>) -> Self {
        let mut mem_dag = MemDag::new();
        mem_dag
            .import_ascii_with_vertex_fn(text, vertex_from_str)
            .unwrap();

        let mut commit_to_tree: HashMap<Vertex, HgId> = Default::default();
        let mut copies: HashMap<Key, Key> = Default::default();
        let tree_store = Arc::new(TestStore::new().with_format(SerializationFormat::Git));
        let changes = Change::build_changes(changes);
        let config: BTreeMap<&'static str, &'static str> = Default::default();

        // iterate through the commit graph to build trees in topo order (ascending)
        let set = mem_dag.all().await.unwrap();
        let mut iter = set.iter_rev().await.unwrap();
        while let Some(item) = iter.next().await {
            Self::build_tree(
                &mem_dag,
                tree_store.clone(),
                &mut commit_to_tree,
                &mut copies,
                item.unwrap(),
                &changes,
            )
            .await;
        }

        let inner = Arc::new(CopyTraceTestCaseInner {
            commit_to_tree,
            tree_store,
            dagalgo: Arc::new(mem_dag),
            copies,
            config,
        });
        CopyTraceTestCase { inner }
    }

    /// Create a DagCopyTrace instance
    pub async fn copy_trace(&self) -> Arc<dyn CopyTrace + Send + Sync> {
        let file_reader = Arc::new(self.clone());
        let config = Arc::new(self.inner.config.clone());
        let rename_finder =
            Arc::new(MetadataRenameFinder::new(file_reader, config.clone()).unwrap());

        let root_tree_reader = Arc::new(self.clone());
        let tree_store = self.inner.tree_store.clone();
        let dagalgo = self.inner.dagalgo.clone();

        let copy_trace =
            DagCopyTrace::new(root_tree_reader, tree_store, rename_finder, dagalgo, config)
                .unwrap();
        Arc::new(copy_trace)
    }

    async fn build_tree(
        dag: &MemDag,
        tree_store: Arc<dyn TreeStore>,
        commit_to_tree: &mut HashMap<Vertex, HgId>,
        copies: &mut HashMap<Key, Key>,
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
                    Change::Modify(path, metadata) => {
                        // `path` should exist
                        tree.get_file(path).unwrap().unwrap();
                        tree.insert(path.clone(), *metadata).unwrap();
                    }
                    Change::Rename(from_path, to_path) | Change::Copy(from_path, to_path) => {
                        let file_metadata = tree.get_file(from_path).unwrap().unwrap();
                        if let Change::Rename(_, _) = v {
                            tree.remove(from_path).unwrap();
                        }
                        tree.insert(to_path.clone(), file_metadata).unwrap();

                        // update copies mapping
                        let to_key = Key::new(to_path.clone(), file_metadata.hgid);
                        let from_key = Key::new(from_path.clone(), file_metadata.hgid);
                        copies.insert(to_key, from_key);
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
impl KeyStore for CopyTraceTestCase {
    fn clone_key_store(&self) -> Box<dyn KeyStore> {
        Box::new(self.clone())
    }
}

#[async_trait]
impl FileStore for CopyTraceTestCase {
    fn get_rename_iter(
        &self,
        keys: Vec<Key>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Key)>>> {
        let store = self.clone();
        let iter = keys
            .into_iter()
            .filter_map(move |k| store.inner.copies.get(&k).cloned().map(|v| Ok((k, v))));
        Ok(Box::new(iter))
    }

    fn clone_file_store(&self) -> Box<dyn FileStore> {
        Box::new(self.clone())
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
            "C" => Change::Copy(to_repo_path_buf(items[1]), to_repo_path_buf(items[2])),
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

macro_rules! assert_trace_rename {
    ($copy_trace:ident $src:tt $dst:tt, $src_path:tt -> $($o:tt)*) => {{
        let src = vertex_from_str(stringify!($src));
        let dst = vertex_from_str(stringify!($dst));
        let src_path = RepoPath::from_str(stringify!($src_path).trim_matches('"'))
            .unwrap()
            .to_owned();
        let expected = stringify!($($o)*).trim_matches('"');
        let expected_result = match &expected[..1] {
            "!" => {
                if expected.len() == 1 {
                    TraceResult::NotFound
                } else {
                    let items: Vec<&str> = expected.split(" ").collect();
                    match items[1] {
                        "-" => TraceResult::Deleted(
                            vertex_from_str(items[2]),
                            RepoPath::from_str(items[3]).unwrap().to_owned()
                        ),
                        "+" => TraceResult::Added(
                            vertex_from_str(items[2]),
                            RepoPath::from_str(items[3]).unwrap().to_owned()),
                        _ => unreachable!(),
                    }
                }
            }
            _ => TraceResult::Renamed(RepoPath::from_str(&expected).unwrap().to_owned()),
        };

        let trace_result = $copy_trace.trace_rename(src, dst, src_path).await.unwrap();
        assert_eq!(trace_result, expected_result);
    }};
}

macro_rules! assert_path_copies {
    ($copy_trace:ident $src:tt $dst:tt, [$( $key:expr => $val:expr ),*]) => {{
        let src = vertex_from_str(stringify!($src));
        let dst = vertex_from_str(stringify!($dst));
        let result = $copy_trace
            .path_copies(src, dst, None)
            .await
            .unwrap()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<HashMap<_, _>>();

        let mut expected = HashMap::new();
        $( expected.insert($key.to_string(), $val.to_string()); )*

        assert_eq!(result, expected);
    }};
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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

    assert_trace_rename!(c A C, a -> b);
    assert_trace_rename!(c C A, b -> a);

    assert_trace_rename!(c A B, a -> b);
    assert_trace_rename!(c B A, b -> a);

    assert_trace_rename!(c A C, d -> !);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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

    assert_trace_rename!(c C B, a -> b);
    assert_trace_rename!(c B C, b -> a);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[traced_test]
async fn test_linear_multiple_renames() {
    let ascii = r#"
    Z
    :
    A
    "#;
    let changes = HashMap::from([
        ("A", vec!["+ a 1"]),
        ("B", vec!["-> a b"]),
        ("H", vec!["-> b c"]),
        ("X", vec!["-> c d"]),
        ("Z", vec!["- d"]),
    ]);
    let t = CopyTraceTestCase::new(ascii, changes).await;
    let c = t.copy_trace().await;

    assert_trace_rename!(c A B, a -> b);
    assert_trace_rename!(c A H, a -> c);
    assert_trace_rename!(c A K, a -> c);
    assert_trace_rename!(c A X, a -> d);

    assert_trace_rename!(c X H, d -> c);
    assert_trace_rename!(c X C, d -> b);
    assert_trace_rename!(c X B, d -> b);
    assert_trace_rename!(c X A, d -> a);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_linear_multiple_renames_with_deletes() {
    let ascii = r#"
    Z
    :
    A
    "#;
    let changes = HashMap::from([
        ("A", vec!["+ a 1"]),
        ("B", vec!["-> a b", "+ b2 2"]),
        ("H", vec!["-> b c"]),
        ("X", vec!["-> c d"]),
        ("Z", vec!["- d"]),
    ]);
    let t = CopyTraceTestCase::new(ascii, changes).await;
    let c = t.copy_trace().await;

    assert_trace_rename!(c A X, a -> d);
    assert_trace_rename!(c A Z, a -> ! - Z d);

    assert_trace_rename!(c Z B, b2 -> b2);
    assert_trace_rename!(c Z A, b2 -> ! + B b2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_non_linear_multiple_renames() {
    let ascii = r#"
    1..10..1023
        \
         A..Z
    "#;
    let changes = HashMap::from([
        ("1", vec!["+ a 1"]),
        ("100", vec!["-> a b"]),
        ("500", vec!["-> b c"]),
        ("1000", vec!["-> c d"]),
        ("C", vec!["M a 11"]),
        ("D", vec!["-> a a2"]),
    ]);
    let t = CopyTraceTestCase::new(ascii, changes).await;
    let c = t.copy_trace().await;

    assert_trace_rename!(c Z 99, a2 -> a);
    assert_trace_rename!(c Z 100, a2 -> b);
    assert_trace_rename!(c Z 101, a2 -> b);
    assert_trace_rename!(c Z 500, a2 -> c);
    assert_trace_rename!(c Z 999, a2 -> c);
    assert_trace_rename!(c Z 1000, a2 -> d);
    assert_trace_rename!(c Z 1001, a2 -> d);
    assert_trace_rename!(c Z 1023, a2 -> d);

    assert_trace_rename!(c C 999, a -> c);
    assert_trace_rename!(c C 1023, a -> d);

    assert_trace_rename!(c 1023 B, d -> a);
    assert_trace_rename!(c 1023 C, d -> a);
    assert_trace_rename!(c 1023 D, d -> a2);
    assert_trace_rename!(c 1023 E, d -> a2);
    assert_trace_rename!(c 1023 Z, d -> a2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_non_linear_multiple_renames_with_deletes() {
    let ascii = r#"
    1..10..1023
        \
         A..Z
    "#;
    let changes = HashMap::from([
        ("1", vec!["+ a 1"]),
        ("100", vec!["-> a b"]),
        ("500", vec!["-> b c"]),
        ("1000", vec!["-> c d"]),
        ("1001", vec!["- d"]),
        ("C", vec!["M a 11"]),
        ("D", vec!["-> a a2"]),
    ]);
    let t = CopyTraceTestCase::new(ascii, changes).await;
    let c = t.copy_trace().await;

    assert_trace_rename!(c Z 1000, a2 -> d);
    assert_trace_rename!(c Z 1001, a2 -> ! - 1001 d);
    assert_trace_rename!(c Z 1023, a2 -> ! - 1001 d);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_multiple_copies_ordering_default() {
    let ascii = r#"
    C
    :
    A
    "#;
    let changes = HashMap::from([("A", vec!["+ a 1"]), ("B", vec!["C a c", "-> a b"])]);
    let t = CopyTraceTestCase::new(ascii, changes).await;
    let c = t.copy_trace().await;

    assert_trace_rename!(c A C, a -> b);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_multiple_copies_ordering_same_basename_win() {
    let ascii = r#"
    C
    :
    A
    "#;
    let changes = HashMap::from([
        ("A", vec!["+ a 1"]),
        ("B", vec!["C a x/b", "C a z/a", "-> a b"]),
    ]);
    let t = CopyTraceTestCase::new(ascii, changes).await;
    let c = t.copy_trace().await;

    assert_trace_rename!(c A C, a -> "z/a");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_multiple_copies_ordering_same_directory_win() {
    let ascii = r#"
    C
    :
    A
    "#;

    let changes = HashMap::from([("A", vec!["+ a 1"]), ("B", vec!["C a x/b", "-> a b"])]);
    let t = CopyTraceTestCase::new(ascii, changes).await;
    let c = t.copy_trace().await;

    assert_trace_rename!(c A C, a -> b);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[traced_test]
async fn test_linear_dir_move() {
    let ascii = r#"
    C
    |
    B
    |
    A
    "#;
    let changes = HashMap::from([
        ("A", vec!["+ a/1.txt 1", "+ a/2.md 2", "+ a/b/3.c 3"]),
        (
            "B",
            vec![
                "-> a/1.txt b/1.txt",
                "-> a/2.md b/2.md",
                "-> a/b/3.c b/b/3.c",
            ],
        ),
        ("C", vec!["M b/1.txt 4"]),
    ]);
    let t = CopyTraceTestCase::new(ascii, changes).await;
    let c = t.copy_trace().await;

    assert_trace_rename!(c A C, "a/1.txt" -> "b/1.txt");
    assert_trace_rename!(c A C, "a/2.md" -> "b/2.md");
    assert_trace_rename!(c C A, "b/1.txt" -> "a/1.txt");
    assert_trace_rename!(c C A, "b/2.md" -> "a/2.md");

    assert_trace_rename!(c A B, "a/b/3.c" -> "b/b/3.c");
    assert_trace_rename!(c B A, "b/b/3.c" -> "a/b/3.c");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_basic_path_copies() {
    let ascii = r#"
    C
    |
    B
    |
    A
    "#;
    let changes = HashMap::from([
        ("A", vec!["+ a/1.txt 1", "+ b/3.c 3"]),
        ("B", vec!["-> a/1.txt b/1.txt", "-> b/3.c b/3.cpp"]),
        ("C", vec!["M b/1.txt 4"]),
    ]);
    let t = CopyTraceTestCase::new(ascii, changes).await;
    let c = t.copy_trace().await;

    assert_path_copies!(c A B, ["b/1.txt" => "a/1.txt", "b/3.cpp" => "b/3.c"]);
    assert_path_copies!(c B A, ["a/1.txt" => "b/1.txt", "b/3.c" => "b/3.cpp"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_forward_copy_then_rename() {
    let ascii = r#"
    C
    |
    B
    |
    A
    "#;
    let changes = HashMap::from([
        ("A", vec!["+ a 1"]),
        ("B", vec!["C a b"]),
        ("C", vec!["-> b c"]),
    ]);
    let t = CopyTraceTestCase::new(ascii, changes).await;
    let c = t.copy_trace().await;

    assert_trace_rename!(c A C, a -> a);
    assert_trace_rename!(c B C, b -> c);
    // should not found, since `b` is not in src commit `A`
    assert_trace_rename!(c A C, b -> !);
}

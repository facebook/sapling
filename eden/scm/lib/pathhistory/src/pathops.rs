/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # pathops
//!
//! Figure out optimal ways to visit multiple paths to extract their
//! content ids. For example, for paths `a/b/x` and `a/b/y`, ideally
//! the common prefix `a/b` is only visited once.
//!
//! The main APIs are `CompiledPaths::compile` and `CompiledPaths::execute`.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use anyhow::Result;
use manifest_tree::FileType;
use manifest_tree::Flag;
use manifest_tree::FsNodeMetadata;
use manifest_tree::TreeEntry;
use manifest_tree::TreeManifest;
use manifest_tree::TreeManifestReadNode;
use manifest_tree::TreeStore;
use types::FetchCause;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;

/// Compiled instructions about how to visit trees to get content of given paths.
pub struct CompiledPaths {
    ops: Vec<Op>,

    /// Provide next ContentIds.
    content_id_map: HashMap<Vec<Option<PathContent>>, ContentId>,

    /// Intern structural content for directories without a persisted ID.
    ephemeral_directory_content_map:
        HashMap<EphemeralDirectoryContent, EphemeralDirectoryContentId>,

    /// Cache to speed up tree lookup.
    lookup_cache: LookupCache,

    /// Cache to speed up durable TreeManifest node lookup.
    manifest_lookup_cache: ManifestLookupCache,

    /// Ignore file content changes if the value is true, this is used
    /// for tracing file renames. Default value is `false`.
    ignore_file_content: bool,
}

// The usize is a pointer to `PathComponentBuf` in `Op`.
// Using `usize` makes borrowck happy.
// This is sound since `ops` in `CompiledPaths` is immutable after being constructed.
type LookupCache = HashMap<(HgId, usize), Option<(HgId, Flag)>>;

// Uses the same stable Op component pointer as LookupCache.
type ManifestLookupCache = HashMap<(HgId, usize), Option<TreeManifestReadNode>>;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum PathContent {
    File { hgid: HgId, file_type: FileType },
    Directory(DirectoryContent),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum DirectoryContent {
    Durable(HgId),
    Ephemeral(EphemeralDirectoryContentId),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct EphemeralDirectoryContentId(usize);

#[derive(Debug, Eq, Hash, PartialEq)]
struct EphemeralDirectoryContent(Vec<EphemeralDirectoryEntry>);

#[derive(Debug, Eq, Hash, PartialEq)]
struct EphemeralDirectoryEntry {
    name: PathComponentBuf,
    path_content: PathContent,
    directory_content: Option<DirectoryContent>,
}

/// A cheap way of comparing `paths`.
#[derive(Debug, Copy, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct ContentId(usize);

/// State machine used to calculate the "ContentId" from a "Root tree"
/// following "Op"s (operations).
struct State<'a> {
    items: Vec<Option<TreeItem<'a>>>,
    output: Vec<Option<PathContent>>,
}

/// An item of a tree. It optionally contains a resolved `TreeEntry`.
#[derive(Debug)]
struct TreeItem<'a> {
    id: HgId,
    flag: Flag,
    path: &'a RepoPath,
    loaded: Option<TreeEntry>,
}

/// Operation that can be applied to `State`.
#[derive(Debug)]
enum Op {
    /// Push the given indexes from the items.
    Output(Vec<usize>),

    /// Look up multiple items. Replace `items` in `State` with lookup result.
    ///
    /// See the `LookupKey`.
    Lookup(Vec<LookupKey>),
}

/// Used in `Op::Lookup`.
struct LookupKey {
    /// Parent tree offset in the `items` vector.
    item_index: usize,

    /// Name to lookup.
    component: PathComponentBuf,

    /// The "redundant" full_path is used as a reference in `TreeItem.path`
    /// to avoid allocation.
    full_path: RepoPathBuf,
}

impl<'a> State<'a> {
    /// Construct `State` from a root tree.
    fn from_root_tree_id(tree_id: HgId) -> Self {
        static ROOT_PATH: RepoPathBuf = RepoPathBuf::new();
        let item = TreeItem {
            id: tree_id,
            flag: Flag::Directory,
            path: &ROOT_PATH,
            loaded: None,
        };
        State {
            items: vec![Some(item)],
            output: Vec::new(),
        }
    }

    /// Convert to output.
    fn into_output(self) -> Vec<Option<PathContent>> {
        if let Some(Some(item)) = self.items.first() {
            tracing::trace!("   root tree {} => {:?}", item.id, &self.output);
        }
        self.output
    }

    /// Execute an operation.
    fn execute(
        &mut self,
        op: &'a Op,
        tree_store: &dyn TreeStore,
        lookup_cache: &mut LookupCache,
    ) -> Result<()> {
        match op {
            Op::Output(indexes) => {
                for &i in indexes {
                    if let Some(item) = self.items.get(i) {
                        let item: Option<&TreeItem> = item.as_ref();
                        let output =
                            item.map(|item| PathContent::from_hgid_flag(item.id, item.flag));
                        self.output.push(output);
                    }
                }
            }
            Op::Lookup(keys) => {
                let mut new_items = Vec::with_capacity(keys.len());
                for key in keys {
                    let new_item = match &self.items[key.item_index] {
                        None => None,
                        Some(item) => {
                            let cache_key =
                                (item.id, &key.component as *const PathComponentBuf as usize);
                            let opt_id_flag = if let Some(value) = lookup_cache.get(&cache_key) {
                                *value
                            } else {
                                let tree_entry =
                                    self.load_tree_entry(tree_store, key.item_index)?;
                                let value = match tree_entry.as_ref() {
                                    Some(tree_entry) => {
                                        tree_entry.elements().lookup(&key.component)?
                                    }
                                    None => None,
                                };
                                lookup_cache.insert(cache_key, value);
                                value
                            };
                            opt_id_flag.map(|(id, flag)| TreeItem {
                                id,
                                flag,
                                path: key.full_path.as_ref(),
                                loaded: None,
                            })
                        }
                    };
                    new_items.push(new_item);
                }
                self.items = new_items;
            }
        }
        Ok(())
    }

    /// Try to load a tree from the tree store.
    fn load_tree_entry(
        &mut self,
        tree_store: &dyn TreeStore,
        item_index: usize,
    ) -> Result<Option<TreeEntry>> {
        let item = match self.items.get_mut(item_index) {
            None | Some(None) => return Ok(None),
            Some(Some(item)) => item,
        };
        if item.flag != Flag::Directory {
            // This happens when the path was a file (or symlink)
            tracing::debug!("   load_tree_entry: {:?} is not a directory", item);
            return Ok(None);
        }
        if item.loaded.is_none() {
            let entry = tree_store.get_content(
                FetchContext::new_with_cause(FetchCause::SaplingLog),
                item.path,
                item.id,
            )?;
            let format = tree_store.format();
            item.loaded = Some(TreeEntry(entry.into_bytes(), format));
        }
        Ok(Some(item.loaded.as_ref().unwrap().clone()))
    }

    /// Append trees that need to be prefetched to `keys`.
    fn push_prefetch_trees(&self, keys: &mut Vec<Key>) {
        for item in self.items.iter() {
            let item = match item.as_ref() {
                Some(item) => item,
                None => continue,
            };
            if item.flag == Flag::Directory && item.loaded.is_none() {
                keys.push(Key::new(item.path.to_owned(), item.id));
            }
        }
    }
}

/// State machine that executes compiled operations over a TreeManifest.
struct ManifestState {
    items: Vec<Option<TreeManifestReadNode>>,
    output: Vec<Option<TreeManifestReadNode>>,
}

impl ManifestState {
    fn from_manifest(manifest: &TreeManifest) -> Result<Self> {
        Ok(Self {
            items: vec![Some(manifest.read_root()?)],
            output: Vec::new(),
        })
    }

    fn into_output(self) -> Vec<Option<TreeManifestReadNode>> {
        self.output
    }

    fn execute(&mut self, op: &Op, lookup_cache: &mut ManifestLookupCache) -> Result<()> {
        match op {
            Op::Output(indexes) => {
                self.output.extend(
                    indexes
                        .iter()
                        .filter_map(|index| self.items.get(*index).cloned()),
                );
            }
            Op::Lookup(keys) => {
                self.items = keys
                    .iter()
                    .map(|key| {
                        let Some(item) = self
                            .items
                            .get(key.item_index)
                            .and_then(|item| item.as_ref())
                        else {
                            return Ok(None);
                        };

                        let cache_key = item
                            .directory_hgid()
                            .map(|hgid| (hgid, &key.component as *const PathComponentBuf as usize));
                        if let Some(value) =
                            cache_key.and_then(|cache_key| lookup_cache.get(&cache_key))
                        {
                            return Ok(value.clone());
                        }

                        let value = item
                            .lookup_children(std::slice::from_ref(&key.component))?
                            .pop()
                            .flatten();
                        if let Some(cache_key) = cache_key {
                            lookup_cache.insert(cache_key, value.clone());
                        }
                        Ok(value)
                    })
                    .collect::<Result<Vec<_>>>()?;
            }
        }
        Ok(())
    }

    fn push_prefetch_nodes(&self, nodes: &mut Vec<TreeManifestReadNode>) {
        nodes.extend(
            self.items
                .iter()
                .flatten()
                .filter(|node| node.directory_hgid().is_some())
                .cloned(),
        );
    }
}

impl CompiledPaths {
    /// Constructs a list of `op`s from a list of paths.
    pub fn compile(mut paths: Vec<RepoPathBuf>) -> Self {
        paths.sort_unstable();
        paths.dedup();

        #[derive(Default)]
        struct Tree {
            full_path: RepoPathBuf,
            entries: BTreeMap<PathComponentBuf, Tree>,
            should_output: bool,
        }

        // Step 1: Store the paths in a virtual tree.
        let mut root = Tree::default();
        for path in &paths {
            let mut tree = &mut root;
            let mut full_path = RepoPathBuf::default();
            for component in path.components() {
                full_path.push(component);
                let subtree = tree
                    .entries
                    .entry(component.to_owned())
                    .or_insert_with(|| Tree {
                        full_path: full_path.clone(),
                        ..Default::default()
                    });
                tree = subtree;
            }
            tree.should_output = true;
        }

        // Step 2: BFS the tree and generate `ops`.
        let mut ops = Vec::new();
        // Trees to visit (within a same depth).
        let mut to_visit = vec![&root];
        // Map from path to `items` index.
        let mut path_indexes: HashMap<RepoPathBuf, usize> =
            std::iter::once((RepoPathBuf::default(), 0)).collect();

        if root.should_output {
            ops.push(Op::Output(vec![0]));
        }

        while !to_visit.is_empty() {
            let mut next_to_visit = Vec::new();
            let mut lookup_keys = Vec::new();
            let mut output_indexes = Vec::new();
            // Prepare Lookup and next_to_visit.
            for tree in &to_visit {
                let item_index = path_indexes[&tree.full_path];
                for (name, subtree) in &tree.entries {
                    if subtree.should_output {
                        output_indexes.push(lookup_keys.len());
                    }
                    lookup_keys.push(LookupKey {
                        item_index,
                        component: name.to_owned(),
                        full_path: subtree.full_path.clone(),
                    });
                    next_to_visit.push(subtree);
                }
            }
            // "Dry-run" the Lookup.
            path_indexes.clear();
            for (i, key) in lookup_keys.iter().enumerate() {
                path_indexes.insert(key.full_path.clone(), i);
            }
            // Append the lookup keys.
            if !lookup_keys.is_empty() {
                ops.push(Op::Lookup(lookup_keys));
                if !output_indexes.is_empty() {
                    ops.push(Op::Output(output_indexes));
                }
            }
            to_visit = next_to_visit;
        }

        tracing::debug!(" compiled pathops: {:?}", &ops);

        Self {
            ops,
            content_id_map: Default::default(),
            ephemeral_directory_content_map: Default::default(),
            lookup_cache: Default::default(),
            manifest_lookup_cache: Default::default(),
            ignore_file_content: Default::default(),
        }
    }

    /// Set the value of ignore_file_content flag.
    ///
    /// When enabled, file content and file type (e.g. executable) will be ignored,
    /// this is used for tracing renames.
    pub fn with_ignore_file_content(mut self, ignore_file_content: bool) -> Self {
        self.ignore_file_content = ignore_file_content;
        self
    }

    /// Convert a list of root tree ids to `ContentId`s that can be compared.
    pub async fn execute(
        &mut self,
        root_tree_ids: Vec<HgId>,
        tree_store: Arc<dyn TreeStore>,
    ) -> Result<Vec<ContentId>> {
        let mut states: Vec<State> = root_tree_ids
            .iter()
            .map(|id| State::from_root_tree_id(*id))
            .collect();

        for op in &self.ops {
            // Prefetch before "Lookup".
            if let Op::Lookup(keys) = op {
                let mut prefetch_keys = Vec::with_capacity(keys.len() * states.len());
                for state in states.iter() {
                    state.push_prefetch_trees(&mut prefetch_keys);
                }
                if !prefetch_keys.is_empty() {
                    prefetch_keys.sort_unstable();
                    prefetch_keys.dedup();
                    let tree_store = tree_store.clone();
                    async_runtime::spawn_blocking(move || tree_store.prefetch(prefetch_keys))
                        .await??;
                }
            }
            // Execute the operation.
            for state in states.iter_mut() {
                async_runtime::block_in_place(|| {
                    state.execute(op, &*tree_store, &mut self.lookup_cache)
                })?;
            }
        }

        let mut result = Vec::with_capacity(root_tree_ids.len());
        for state in states {
            result.push(intern_output(
                &mut self.content_id_map,
                self.ignore_file_content,
                state.into_output(),
            ));
        }
        Ok(result)
    }

    /// Convert manifests to `ContentId`s using the same compiled path operations as `execute`.
    pub async fn execute_manifests(
        &mut self,
        manifests: Vec<TreeManifest>,
    ) -> Result<Vec<ContentId>> {
        let mut states = manifests
            .iter()
            .map(ManifestState::from_manifest)
            .collect::<Result<Vec<_>>>()?;

        for op in &self.ops {
            if let Op::Lookup(_) = op {
                let mut nodes = Vec::new();
                states
                    .iter()
                    .for_each(|state| state.push_prefetch_nodes(&mut nodes));
                if !nodes.is_empty() {
                    async_runtime::block_on(async { TreeManifestReadNode::prefetch(&nodes) })?;
                }
            }

            for state in &mut states {
                async_runtime::block_on(async {
                    state.execute(op, &mut self.manifest_lookup_cache)
                })?;
            }
        }

        let mut ephemeral_cache = HashMap::new();
        states
            .into_iter()
            .enumerate()
            .map(|(manifest_index, state)| {
                let output = state
                    .into_output()
                    .into_iter()
                    .map(|node| {
                        node.as_ref()
                            .map(|node| {
                                self.path_content_for_node(
                                    manifest_index,
                                    node,
                                    &mut ephemeral_cache,
                                )
                            })
                            .transpose()
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(intern_output(
                    &mut self.content_id_map,
                    self.ignore_file_content,
                    output,
                ))
            })
            .collect()
    }

    fn path_content_for_node(
        &mut self,
        manifest_index: usize,
        node: &TreeManifestReadNode,
        ephemeral_cache: &mut HashMap<(usize, RepoPathBuf), DirectoryContent>,
    ) -> Result<PathContent> {
        match node.metadata() {
            FsNodeMetadata::File(metadata) => Ok(PathContent::File {
                hgid: metadata.hgid,
                file_type: metadata.file_type,
            }),
            FsNodeMetadata::Directory(_) => Ok(PathContent::Directory(
                self.directory_content_for_node(manifest_index, node, ephemeral_cache)?,
            )),
        }
    }

    fn directory_content_for_node(
        &mut self,
        manifest_index: usize,
        node: &TreeManifestReadNode,
        ephemeral_cache: &mut HashMap<(usize, RepoPathBuf), DirectoryContent>,
    ) -> Result<DirectoryContent> {
        if let Some(hgid) = node.directory_hgid() {
            return Ok(DirectoryContent::Durable(hgid));
        }
        if !node.is_directory() {
            anyhow::bail!("{} is not a directory", node.path());
        }

        let cache_key = (manifest_index, node.path().to_owned());
        if let Some(content) = ephemeral_cache.get(&cache_key) {
            return Ok(*content);
        }

        let mut entries = Vec::new();
        for (name, child) in node.children()? {
            let directory_content = if child.is_directory() {
                Some(self.directory_content_for_node(manifest_index, &child, ephemeral_cache)?)
            } else {
                None
            };
            let path_content = match child.metadata() {
                FsNodeMetadata::File(metadata) => PathContent::File {
                    hgid: metadata.hgid,
                    file_type: metadata.file_type,
                },
                FsNodeMetadata::Directory(_) => {
                    PathContent::Directory(directory_content.ok_or_else(|| {
                        anyhow::anyhow!("{} has directory metadata without children", child.path())
                    })?)
                }
            };
            entries.push(EphemeralDirectoryEntry {
                name,
                path_content,
                directory_content,
            });
        }

        let next_id = EphemeralDirectoryContentId(self.ephemeral_directory_content_map.len() + 1);
        let id = *self
            .ephemeral_directory_content_map
            .entry(EphemeralDirectoryContent(entries))
            .or_insert(next_id);
        let content = DirectoryContent::Ephemeral(id);
        ephemeral_cache.insert(cache_key, content);
        Ok(content)
    }
}

fn intern_output(
    content_id_map: &mut HashMap<Vec<Option<PathContent>>, ContentId>,
    ignore_file_content: bool,
    mut output: Vec<Option<PathContent>>,
) -> ContentId {
    if ignore_file_content {
        output
            .iter_mut()
            .for_each(normalize_content_key_ignore_file_content);
    }
    if output.iter().all(Option::is_none) {
        ContentId::empty()
    } else {
        let next_content_id = ContentId(content_id_map.len() + 1);
        *content_id_map.entry(output).or_insert(next_content_id)
    }
}

fn normalize_content_key_ignore_file_content(optional_key: &mut Option<PathContent>) {
    match optional_key {
        Some(PathContent::File { hgid, file_type }) => {
            *hgid = *HgId::wdir_id();
            *file_type = FileType::Regular;
        }
        // Directory content changes when a file in the directory changes.
        // Keep only the existence of the directory.
        Some(PathContent::Directory(content)) => {
            *content = DirectoryContent::Durable(*HgId::wdir_id());
        }
        None => {}
    }
}

impl PathContent {
    fn from_hgid_flag(hgid: HgId, flag: Flag) -> Self {
        match flag {
            Flag::File(file_type) => Self::File { hgid, file_type },
            Flag::Directory => Self::Directory(DirectoryContent::Durable(hgid)),
        }
    }
}

impl ContentId {
    /// Empty `ContentId`: all paths are missing.
    pub fn empty() -> Self {
        Self(0)
    }

    /// Test if the `ContentId` is empty.
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }
}

// Customized Debug impl that is a bit shorter.
impl fmt::Debug for LookupKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let full_path = self.full_path.as_str();
        let component = self.component.as_str();
        write!(
            f,
            "{}:{}<{}>",
            self.item_index,
            &full_path[..(full_path.len() - component.len())],
            component,
        )
    }
}

#[cfg(test)]
mod tests {
    use manifest_tree::FileMetadata;
    use manifest_tree::Manifest;
    use manifest_tree::PathComponent;
    use manifest_tree::PathTranslator;
    use manifest_tree::testutil::TestStore;
    use manifest_tree::testutil::make_tree_manifest;
    use types::testutil::hgid;

    use super::*;

    #[derive(Debug)]
    struct SuffixPathTranslator;

    impl PathTranslator for SuffixPathTranslator {
        fn encode_file(&self, path: &RepoPath) -> Result<RepoPathBuf> {
            Ok(RepoPathBuf::from_string(format!("{}\x7f", path.as_str()))?)
        }

        fn decode_file(&self, path: &RepoPath) -> Result<RepoPathBuf> {
            let decoded = path.as_str().strip_suffix('\x7f').unwrap_or(path.as_str());
            Ok(RepoPathBuf::from_string(decoded.to_owned())?)
        }

        fn encode_file_name(&self, name: &PathComponent) -> Result<PathComponentBuf> {
            Ok(PathComponentBuf::from_string(format!(
                "{}\x7f",
                name.as_str()
            ))?)
        }

        fn decode_file_name(&self, name: &PathComponent) -> Result<PathComponentBuf> {
            let decoded = name.as_str().strip_suffix('\x7f').unwrap_or(name.as_str());
            Ok(PathComponentBuf::from_string(decoded.to_owned())?)
        }
    }

    fn durable_manifest(
        store: Arc<TestStore>,
        paths: &[(&str, &str)],
    ) -> Result<(HgId, TreeManifest)> {
        let mut manifest = make_tree_manifest(store.clone(), paths);
        let root_hgid = manifest.persist(&[])?;
        Ok((root_hgid, TreeManifest::durable(store, root_hgid)))
    }

    fn overlapping_manifest(store: Arc<TestStore>, child_hgid: HgId) -> Result<TreeManifest> {
        let mut manifest = TreeManifest::ephemeral(store);
        manifest.set_path_translator(Arc::new(SuffixPathTranslator));
        manifest.insert(
            p("a"),
            FileMetadata::new(hgid("10"), FileType::GitSubmodule),
        )?;
        manifest.insert(
            p("a/b"),
            FileMetadata::new(child_hgid, FileType::GitSubmodule),
        )?;
        Ok(manifest)
    }

    #[test]
    fn test_op_compile() {
        let t = |v: &[&str]| {
            CompiledPaths::compile(v.iter().map(|s| p(s)).collect::<Vec<_>>())
                .ops
                .into_iter()
                .map(|s| format!("{s:?}"))
                .collect::<Vec<String>>()
        };

        // Single path
        assert_eq!(t(&[]), [] as [&str; 0]);
        assert_eq!(t(&[""]), ["Output([0])"]);
        assert_eq!(t(&["a"]), ["Lookup([0:<a>])", "Output([0])"]);
        assert_eq!(
            t(&["a/b/c"]),
            [
                "Lookup([0:<a>])",
                "Lookup([0:a/<b>])",
                "Lookup([0:a/b/<c>])",
                "Output([0])"
            ]
        );

        // Multiple paths
        assert_eq!(
            t(&["a/b/c", "a"]),
            [
                "Lookup([0:<a>])",
                "Output([0])",
                "Lookup([0:a/<b>])",
                "Lookup([0:a/b/<c>])",
                "Output([0])"
            ]
        );
        assert_eq!(
            t(&["a/b/c/d", "a/b/c/e"]),
            [
                "Lookup([0:<a>])",
                "Lookup([0:a/<b>])",
                "Lookup([0:a/b/<c>])",
                "Lookup([0:a/b/c/<d>, 0:a/b/c/<e>])",
                "Output([0, 1])"
            ]
        );
        assert_eq!(
            t(&["a/b/c", "a/d/e", "x/y"]),
            [
                "Lookup([0:<a>, 0:<x>])",
                "Lookup([0:a/<b>, 0:a/<d>, 1:x/<y>])",
                "Output([2])",
                "Lookup([0:a/b/<c>, 1:a/d/<e>])",
                "Output([0, 1])"
            ]
        );
        assert_eq!(
            t(&["a/b", "d", "e/f/g/h", "x/y"]),
            [
                "Lookup([0:<a>, 0:<d>, 0:<e>, 0:<x>])",
                "Output([1])",
                "Lookup([0:a/<b>, 2:e/<f>, 3:x/<y>])",
                "Output([0, 2])",
                "Lookup([1:e/f/<g>])",
                "Lookup([0:e/f/g/<h>])",
                "Output([0])"
            ]
        );
    }

    #[test]
    fn test_normalize_content_key_ignore_file_content() {
        let normalize = |mut key: Option<PathContent>| {
            normalize_content_key_ignore_file_content(&mut key);
            key
        };
        let hgid = HgId::from_byte_array([1; HgId::len()]);
        let wdir_id = *HgId::wdir_id();

        assert_eq!(
            normalize(Some(PathContent::File {
                hgid,
                file_type: FileType::Executable,
            })),
            Some(PathContent::File {
                hgid: wdir_id,
                file_type: FileType::Regular,
            })
        );
        assert_eq!(
            normalize(Some(PathContent::Directory(DirectoryContent::Durable(
                hgid
            )))),
            Some(PathContent::Directory(DirectoryContent::Durable(wdir_id)))
        );
        assert_eq!(
            normalize(Some(PathContent::Directory(DirectoryContent::Ephemeral(
                EphemeralDirectoryContentId(1)
            )))),
            Some(PathContent::Directory(DirectoryContent::Durable(wdir_id)))
        );
        assert_eq!(normalize(None), None);
    }

    #[test]
    fn test_execute_manifests_matches_tree_store() -> Result<()> {
        async_runtime::block_on(async {
            let store = Arc::new(TestStore::new());
            let (first_root, first) =
                durable_manifest(store.clone(), &[("a/b", "10"), ("unrelated", "20")])?;
            let (second_root, second) =
                durable_manifest(store.clone(), &[("a/b", "10"), ("unrelated", "30")])?;
            let (third_root, third) =
                durable_manifest(store.clone(), &[("a/b", "11"), ("unrelated", "30")])?;
            let paths = vec![p("a"), p("a/b"), p("missing")];

            let mut legacy = CompiledPaths::compile(paths.clone());
            let tree_store: Arc<dyn TreeStore> = store;
            let legacy_ids = legacy
                .execute(vec![first_root, second_root, third_root], tree_store)
                .await?;
            let mut manifest = CompiledPaths::compile(paths);
            let manifest_ids = manifest
                .execute_manifests(vec![first, second, third])
                .await?;

            assert_eq!(manifest_ids, legacy_ids);
            assert_eq!(manifest_ids[0], manifest_ids[1]);
            assert_ne!(manifest_ids[1], manifest_ids[2]);
            Ok(())
        })
    }

    #[test]
    fn test_execute_manifests_interns_ephemeral_directory_content() -> Result<()> {
        async_runtime::block_on(async {
            let store = Arc::new(TestStore::new());
            let manifests = vec![
                make_tree_manifest(store.clone(), &[("dir/file", "10")]),
                make_tree_manifest(store.clone(), &[("dir/file", "10")]),
                make_tree_manifest(store, &[("dir/file", "20")]),
            ];
            let mut paths = CompiledPaths::compile(vec![p("dir")]);

            let ids = paths.execute_manifests(manifests).await?;

            assert_eq!(ids[0], ids[1]);
            assert_ne!(ids[1], ids[2]);
            Ok(())
        })
    }

    #[test]
    fn test_execute_manifests_traverses_overlapping_file_and_directory() -> Result<()> {
        async_runtime::block_on(async {
            let store = Arc::new(TestStore::new());
            let first = overlapping_manifest(store.clone(), hgid("20"))?;
            let second = overlapping_manifest(store, hgid("30"))?;

            let mut exact_path = CompiledPaths::compile(vec![p("a")]);
            let exact_ids = exact_path
                .execute_manifests(vec![first.clone(), second.clone()])
                .await?;
            let mut child_path = CompiledPaths::compile(vec![p("a/b")]);
            let child_ids = child_path.execute_manifests(vec![first, second]).await?;

            assert_eq!(exact_ids[0], exact_ids[1]);
            assert_ne!(child_ids[0], child_ids[1]);
            Ok(())
        })
    }

    fn p(s: &str) -> RepoPathBuf {
        RepoPathBuf::from_string(s.to_string()).unwrap()
    }
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
use manifest_tree::Flag;
use manifest_tree::TreeEntry;
use manifest_tree::TreeStore;
use types::HgId;
use types::Key;
use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;

/// Compiled instructions about how to visit trees to get content of given paths.
pub struct CompiledPaths {
    ops: Vec<Op>,

    /// Provide next ContentIds.
    content_id_map: HashMap<Vec<Option<(HgId, Flag)>>, ContentId>,

    /// Cache to speed up tree lookup.
    lookup_cache: LookupCache,
}

// The usize is a pointer to `PathComponentBuf` in `Op`.
// Using `usize` makes borrowck happy.
// This is sound since `ops` in `CompiledPaths` is immutable after being constructed.
type LookupCache = HashMap<(HgId, usize), Option<(HgId, Flag)>>;

/// A cheap way of comparing `paths`.
#[derive(Debug, Copy, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct ContentId(usize);

/// State machine used to calculate the "ContentId" from a "Root tree"
/// following "Op"s (operations).
struct State<'a> {
    items: Vec<Option<TreeItem<'a>>>,
    output: Vec<Option<(HgId, Flag)>>,
}

/// An item of a tree. It optionally contains a resolved `TreeEntry`.
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
    fn into_output(self) -> Vec<Option<(HgId, Flag)>> {
        if let Some(Some(item)) = self.items.get(0) {
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
                        let output: Option<(HgId, Flag)> = item.map(|f| (f.id, f.flag));
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
        if item.loaded.is_none() {
            let entry = tree_store.get(item.path, item.id)?;
            let format = tree_store.format();
            item.loaded = Some(TreeEntry(entry, format));
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

        // Step 2: BFS the tree and genearte `ops`.
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
            lookup_cache: Default::default(),
        }
    }

    /// Convert a list of root tree ids to `ContentId`s that can be compared.
    pub async fn execute(
        &mut self,
        root_tree_ids: Vec<HgId>,
        tree_store: Arc<dyn TreeStore + Send + Sync>,
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
                state.execute(op, &*tree_store, &mut self.lookup_cache)?;
            }
        }

        let mut result = Vec::with_capacity(root_tree_ids.len());
        for state in states {
            let output = state.into_output();
            let content_id = if output.iter().all(|o| o.is_none()) {
                ContentId::empty()
            } else {
                let next_content_id = ContentId(self.content_id_map.len() + 1);
                *self.content_id_map.entry(output).or_insert(next_content_id)
            };
            result.push(content_id);
        }
        Ok(result)
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
    use super::*;

    #[test]
    fn test_op_compile() {
        let t = |v: &[&str]| {
            CompiledPaths::compile(v.iter().map(|s| p(s)).collect::<Vec<_>>())
                .ops
                .into_iter()
                .map(|s| format!("{:?}", s))
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

    fn p(s: &str) -> RepoPathBuf {
        RepoPathBuf::from_string(s.to_string()).unwrap()
    }
}

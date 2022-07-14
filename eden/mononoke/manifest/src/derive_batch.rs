/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::Entry;
use crate::LeafInfo;
use crate::Manifest;
use crate::PathTree;
use crate::TreeInfo;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use blobstore::StoreLoadable;
use cloned::cloned;
use context::CoreContext;
use futures::Future;
use futures::FutureExt;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use std::collections::BTreeMap;
use std::fmt;
use std::hash::Hash;
use std::sync::Arc;

pub struct ManifestChanges<Leaf> {
    pub cs_id: ChangesetId,
    pub changes: Vec<(MPath, Option<Leaf>)>,
}

// Function that can derive manifests for a "simple" stack of commits. But what does "simple" mean?
// In this case "simple" means:
// 1) There are no merges i.e. we get just a Linear stack of commits
//    (`changes` parameter  should be sorted from ancestors to descendant)
// 2) Paths that commits modify should not be prefixes of each other i.e.
//    stack shouldn't have file changes ("dir/A" => "dir") and ("dir" => "file").
//    The exception applies only to changes that were modified in the same commit
//    i.e. if a single commit has ("dir" => None) and ("dir/A" => "content") then this
//    commit can be derived by derive_manifests_for_simple_stack_of_commits.
#[allow(unused)]
pub async fn derive_manifests_for_simple_stack_of_commits<
    'caller,
    TreeId,
    LeafId,
    IntermediateLeafId,
    Leaf,
    T,
    TFut,
    L,
    LFut,
    Ctx,
    Store,
>(
    ctx: CoreContext,
    store: Store,
    parent: Option<TreeId>,
    changes: Vec<ManifestChanges<Leaf>>,
    create_tree: T,
    create_leaf: L,
) -> Result<BTreeMap<ChangesetId, TreeId>, Error>
where
    Store: Sync + Send + Clone + 'static,
    LeafId: Send + Clone + Eq + Hash + fmt::Debug + 'static + Sync,
    IntermediateLeafId: Clone + Send + From<LeafId> + 'static + Sync,
    Leaf: Send + 'static,
    TreeId: StoreLoadable<Store> + Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
    TreeId::Value: Manifest<TreeId = TreeId, LeafId = LeafId> + Sync,
    <TreeId as StoreLoadable<Store>>::Value: Send,
    T: Fn(TreeInfo<TreeId, IntermediateLeafId, Ctx>, ChangesetId) -> TFut + Send + Sync + 'static,
    TFut: Future<Output = Result<(Ctx, TreeId), Error>> + Send + 'caller,
    L: Fn(LeafInfo<IntermediateLeafId, Leaf>, ChangesetId) -> LFut + Send + Sync + 'static,
    LFut: Future<Output = Result<(Ctx, IntermediateLeafId), Error>> + Send + 'caller,
    Ctx: Clone + Send + Sync + 'static,
{
    Deriver {
        ctx,
        store,
        parent,
        changes,
        create_tree,
        create_leaf,
        _marker: std::marker::PhantomData,
    }
    .derive()
    .await
}

// Stack of changes for a single file path - it can be either a tree or a leaf
struct EntryStack<TreeId, LeafId, Ctx> {
    parent: Option<Entry<TreeId, LeafId>>,
    values: Vec<(ChangesetId, Option<Ctx>, Option<Entry<TreeId, LeafId>>)>,
}

// This struct is not necessary, it just exists so that we don't need to repeat
// a long list of generic restrictions for each function
struct Deriver<TreeId, Leaf, IntermediateLeafId, T, L, Store> {
    ctx: CoreContext,
    store: Store,
    parent: Option<TreeId>,
    changes: Vec<ManifestChanges<Leaf>>,
    create_tree: T,
    create_leaf: L,
    _marker: std::marker::PhantomData<IntermediateLeafId>,
}

fn convert_to_intermediate_entry<TreeId, LeafId, IntermediateLeafId>(
    e: Entry<TreeId, LeafId>,
) -> Entry<TreeId, IntermediateLeafId>
where
    IntermediateLeafId: From<LeafId>,
{
    match e {
        Entry::Tree(t) => Entry::Tree(t),
        Entry::Leaf(l) => Entry::Leaf(l.into()),
    }
}

impl<'caller, TreeId, LeafId, IntermediateLeafId, Leaf, T, TFut, L, LFut, Ctx, Store>
    Deriver<TreeId, Leaf, IntermediateLeafId, T, L, Store>
where
    Store: Sync + Send + Clone + 'static,
    LeafId: Send + Clone + Eq + Hash + fmt::Debug + 'static + Sync,
    IntermediateLeafId: Clone + Send + From<LeafId> + 'static + Sync,
    Leaf: Send + 'static,
    TreeId: StoreLoadable<Store> + Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
    TreeId::Value: Manifest<TreeId = TreeId, LeafId = LeafId> + Sync,
    <TreeId as StoreLoadable<Store>>::Value: Send,
    T: Fn(TreeInfo<TreeId, IntermediateLeafId, Ctx>, ChangesetId) -> TFut + Send + Sync + 'static,
    TFut: Future<Output = Result<(Ctx, TreeId), Error>> + Send + 'caller,
    L: Fn(LeafInfo<IntermediateLeafId, Leaf>, ChangesetId) -> LFut + Send + Sync + 'static,
    LFut: Future<Output = Result<(Ctx, IntermediateLeafId), Error>> + Send + 'caller,
    Ctx: Clone + Send + Sync + 'static,
{
    async fn derive(self) -> Result<BTreeMap<ChangesetId, TreeId>, Error> {
        let Deriver {
            ctx,
            store,
            parent,
            changes,
            create_tree,
            create_leaf,
            ..
        } = self;

        // This is a tree of paths, and for each path we store a list of
        // (ChangesetId, Option<Leaf>) i.e. for each path we store
        // changesets where it was modified and what this modification was.
        // Each list of modifications is ordered in the same order as the commits
        // in the stack.
        let mut path_tree = PathTree::<Vec<(ChangesetId, Option<Leaf>)>>::default();

        let mut stack_of_commits = vec![];
        for mf_changes in changes {
            for (path, leaf) in mf_changes.changes {
                path_tree.insert_and_merge(Some(path), (mf_changes.cs_id, leaf));
            }
            stack_of_commits.push(mf_changes.cs_id);
        }

        struct UnfoldState<TreeId, LeafId, Leaf> {
            path: Option<MPath>,
            name: Option<MPathElement>,
            parent: Option<Entry<TreeId, LeafId>>,
            path_tree: PathTree<Vec<(ChangesetId, Leaf)>>,
        }

        enum FoldState<TreeId, LeafId, Leaf> {
            Reuse(
                Option<MPath>,
                Option<MPathElement>,
                Option<Entry<TreeId, LeafId>>,
            ),
            CreateLeaves(
                Option<MPath>,
                MPathElement,
                Option<Entry<TreeId, LeafId>>,
                Vec<Leaf>,
            ),
            CreateTrees(
                Option<MPath>,
                Option<MPathElement>,
                Option<Entry<TreeId, LeafId>>,
                // We might have a single file deletion, this field represents it
                Option<ChangesetId>,
            ),
        }

        let stack_of_commits = Arc::new(stack_of_commits);
        let (_, entry_stack) = bounded_traversal::bounded_traversal(
            256,
            UnfoldState {
                path: None,
                name: None,
                parent: parent.clone().map(Entry::Tree),
                path_tree,
            },
            // Unfold - during this traversal we visit all changed paths and decide what to do with them
            {
                cloned!(ctx, store);
                move |
                    UnfoldState {
                        path,
                        name,
                        parent,
                        path_tree,
                    },
                | {
                    cloned!(ctx, store);
                    async move {
                        let PathTree {
                            value: changes,
                            subentries,
                        } = path_tree;

                        if !changes.is_empty() && subentries.is_empty() {
                            // We have a stack of changes for a given leaf
                            let name = name.ok_or_else(|| anyhow!("unexpected empty path for leaf"))?;
                            Ok((FoldState::CreateLeaves(path, name, parent, changes), vec![]))
                        } else if !subentries.is_empty() {
                            // We need to recurse now - recurse into all parent entries and all subentries

                            let maybe_file_deletion = if !changes.is_empty() {
                                // So a file was changed and also some other subdirectories were
                                // changed. In most cases that would just mean that we are not dealing
                                // with the simple stack and we should just error out, however there's
                                // one exception. If a file was replaced with a directory in a single
                                // commit then we might have both deletion of a file and a few
                                // subentries with the same prefix (e.g. "R file; A file/subentry").
                                // Let's check if that's indeed the case.

                                let mut only_single_deletion = None;
                                if changes.len() == 1 {
                                    let (cs_id, file_change) = &changes[0];
                                    if file_change.is_none() {
                                        only_single_deletion = Some(*cs_id);
                                    }
                                }

                                if only_single_deletion.is_none() {
                                    return Err(anyhow!(
                                        "unsupported stack derive - {:?} is a prefix of other paths",
                                        path
                                    ));
                                }

                                only_single_deletion
                            } else {
                                None
                            };

                            let mut deps: BTreeMap<MPathElement, _> = Default::default();
                            if let Some(Entry::Tree(tree_id)) = &parent {
                                let mf = tree_id.load(&ctx, &store).await?;
                                for (name, entry) in mf.list() {
                                    let subentry =
                                        deps.entry(name.clone()).or_insert_with(|| UnfoldState {
                                            path: Some(MPath::join_opt_element(path.as_ref(), &name)),
                                            name: Some(name),
                                            parent: Default::default(),
                                            path_tree: Default::default(),
                                        });
                                    subentry.parent = Some(entry);
                                }
                            }

                            for (name, path_tree) in subentries {
                                let subentry =
                                    deps.entry(name.clone()).or_insert_with(|| UnfoldState {
                                        path: Some(MPath::join_opt_element(path.as_ref(), &name)),
                                        name: Some(name),
                                        parent: Default::default(),
                                        path_tree: Default::default(),
                                    });
                                subentry.path_tree = path_tree;
                            }

                            let deps = deps.into_iter().map(|(_name, dep)| dep).collect();
                            Ok((
                                FoldState::CreateTrees(path, name, parent, maybe_file_deletion),
                                deps,
                            ))
                        } else if path.is_none() && parent.is_none() {
                            // This is a weird case - we got an empty commit with no parent.
                            // In that case  we want to create an empty root tree for this commit
                            Ok((FoldState::CreateTrees(None, None, None, None), vec![]))
                        } else {
                            // No changes, no subentries - just reuse the entry
                            Ok((FoldState::Reuse(path, name, parent.map(convert_to_intermediate_entry)), vec![]))
                        }
                    }
                    .boxed()
                }
            },
            // Fold - actually create the entries
            {
                let create_leaf = Arc::new(create_leaf);
                let create_tree = Arc::new(create_tree);
                cloned!(ctx, store, stack_of_commits);
                move |fold_state, subentries| {
                    cloned!(ctx, create_leaf, create_tree, stack_of_commits, store);
                    async move {
                        let subentries: BTreeMap<MPathElement, _> = subentries
                            .filter_map(|(maybe_path, val): (Option<_>, _)| {
                                maybe_path.map(|path| (path, val))
                            })
                            .collect();

                        match fold_state {
                            FoldState::CreateLeaves(path, name, parent, leaves) => {
                                if !subentries.is_empty() {
                                    anyhow::bail!(
                                        "Can't create entries for {:?} - have unexpected subentries",
                                        path,
                                    );
                                }

                                let path = path.clone().context("unexpected empty path for leaf")?;
                                let entry_stack = Self::create_leaves(
                                    &ctx,
                                    path,
                                    parent.map(convert_to_intermediate_entry),
                                    leaves,
                                    create_leaf,
                                    create_tree,
                                    store,
                                )
                                .await?;
                                Ok((Some(name), entry_stack))
                            }
                            FoldState::CreateTrees(path, name, parent, maybe_file_deletion) => {
                                let entry_stack = Self::create_trees(
                                    path,
                                    parent.map(convert_to_intermediate_entry),
                                    subentries,
                                    create_tree,
                                    stack_of_commits,
                                    maybe_file_deletion,
                                )
                                .await?;
                                Ok((name, entry_stack))
                            }
                            FoldState::Reuse(_, name, maybe_entry) => {
                                let entry_stack = EntryStack {
                                    parent: maybe_entry.map(convert_to_intermediate_entry),
                                    values: vec![],
                                };
                                Ok((name, entry_stack))
                            }
                        }
                    }
                    .boxed()
                }
            },
        )
        .await?;

        let derived: BTreeMap<_, _> = entry_stack
            .values
            .into_iter()
            .filter_map(|(cs_id, _, maybe_entry)| {
                let maybe_tree_id = match maybe_entry {
                    Some(entry) => Some(entry.into_tree()?),
                    None => None,
                };

                Some((cs_id, maybe_tree_id))
            })
            .collect();

        let mut parent = parent;
        // Make sure that we have entries for every commit
        let mut res = BTreeMap::new();
        for cs_id in stack_of_commits.iter() {
            let new_mf_id = match derived.get(cs_id) {
                Some(maybe_tree_id) => {
                    parent = maybe_tree_id.clone();
                    maybe_tree_id.clone()
                }
                None => parent.clone(),
            };
            let new_mf_id =
                new_mf_id.ok_or_else(|| anyhow!("unexpected empty manifest for {}", cs_id))?;
            res.insert(*cs_id, new_mf_id);
        }

        Ok(res)
    }

    async fn create_leaves(
        ctx: &CoreContext,
        path: MPath,
        parent: Option<Entry<TreeId, IntermediateLeafId>>,
        changes: Vec<(ChangesetId, Option<Leaf>)>,
        create_leaf: Arc<L>,
        create_tree: Arc<T>,
        store: Store,
    ) -> Result<EntryStack<TreeId, IntermediateLeafId, Ctx>, Error> {
        let mut entry_stack = EntryStack {
            parent: parent.clone(),
            values: vec![],
        };
        let mut parent = parent;

        for (cs_id, maybe_leaf) in changes {
            match maybe_leaf {
                Some(leaf) => {
                    let (upload_ctx, leaf_id) = create_leaf(
                        LeafInfo {
                            leaf: Some(leaf),
                            path: path.clone(),
                            // Note that a parent can be a tree here - in that case
                            // directory is implicitly deleted and replaced with a file
                            parents: parent.and_then(Entry::into_leaf).into_iter().collect(),
                        },
                        cs_id,
                    )
                    .await?;

                    entry_stack.values.push((
                        cs_id,
                        Some(upload_ctx),
                        Some(Entry::Leaf(leaf_id.clone())),
                    ));
                    parent = Some(Entry::Leaf(leaf_id));
                }
                None => {
                    if let Some(Entry::Tree(tree_id)) = parent {
                        // This is a weird logic in derive_manifest()
                        // If file is deleted and parent entry is a tree then
                        // we ignore the deletion and create a new object with
                        // existing parent tree entry as a parent.
                        // This is strange, but it's worth replicating what we have
                        // derive_manifest()
                        let parent_mf = tree_id.load(ctx, &store).await?;
                        let subentries = parent_mf
                            .list()
                            .map(|(path, entry)| {
                                (path, (None, convert_to_intermediate_entry(entry)))
                            })
                            .collect();
                        let (upload_ctx, tree_id) = create_tree(
                            TreeInfo {
                                path: Some(path.clone()),
                                parents: vec![tree_id],
                                subentries,
                            },
                            cs_id,
                        )
                        .await?;
                        entry_stack.values.push((
                            cs_id,
                            Some(upload_ctx),
                            Some(Entry::Tree(tree_id.clone())),
                        ));
                        parent = Some(Entry::Tree(tree_id));
                    } else {
                        entry_stack.values.push((cs_id, None, None));
                        parent = None;
                    }
                }
            }
        }

        Ok(entry_stack)
    }

    async fn create_trees(
        path: Option<MPath>,
        parent: Option<Entry<TreeId, IntermediateLeafId>>,
        stack_sub_entries: BTreeMap<MPathElement, EntryStack<TreeId, IntermediateLeafId, Ctx>>,
        create_tree: Arc<T>,
        stack_of_commits: Arc<Vec<ChangesetId>>,
        maybe_file_deletion: Option<ChangesetId>,
    ) -> Result<EntryStack<TreeId, IntermediateLeafId, Ctx>, Error> {
        // These are all sub entries for the commit we are currently processing.
        // We start with parent entries, and then apply delta changes on top.
        let mut cur_sub_entries: BTreeMap<MPathElement, (Option<Ctx>, Entry<_, _>)> =
            BTreeMap::new();

        // `stack_sub_entries` is a mapping from (name -> list of changes).
        // We want to pivot it into (Changeset id -> Map(name, entry)),
        // so that we know how tree directory changed
        let mut delta_sub_entries: BTreeMap<ChangesetId, BTreeMap<_, _>> = BTreeMap::new();

        for (path_elem, entry_stack) in stack_sub_entries {
            let EntryStack { values, parent } = entry_stack;
            if let Some(parent) = parent {
                cur_sub_entries.insert(path_elem.clone(), (None, parent));
            }

            for (cs_id, ctx, maybe_entry) in values {
                delta_sub_entries
                    .entry(cs_id)
                    .or_default()
                    .insert(path_elem.clone(), (ctx, maybe_entry));
            }
        }

        // Before we start with the actual logic of creating trees we have one corner case
        // to deal with. A file can be replaced with a directory in a single commit. In that
        // case we have a maybe_file_deletion set to the the commit where the file was deleted,
        // and a parent should either be non-existent or a leaf. A case like that can only be
        // the first in the stack of changes this tree because we use a simple stack.
        // So let's check these two things if file deletion is set:
        // 1) Check that parent is a leaf or non-existent
        // 2) Check that deletion is the first change
        match (&parent, maybe_file_deletion) {
            (Some(Entry::Leaf(_)), Some(deletion_cs_id)) | (None, Some(deletion_cs_id)) => {
                for cs_id in stack_of_commits.iter() {
                    if cs_id == &deletion_cs_id {
                        // File is deleted in this commit, and no previous commits had subentries.
                        // That's good, we can exit
                        break;
                    }

                    // There are subentries before the deletion - we don't support this, so
                    // let's exit.
                    if delta_sub_entries.contains_key(cs_id) {
                        return Err(anyhow!(
                            "Unexpected file deletion of {:?} in {}",
                            path,
                            deletion_cs_id
                        ));
                    }
                }
            }
            (Some(Entry::Tree(_)), Some(deletion_cs_id)) => {
                // Something is odd here - parent is a tree but we try to delete it as a file
                return Err(anyhow!(
                    "Unexpected file deletion of {:?} in {}",
                    path,
                    deletion_cs_id
                ));
            }
            (Some(Entry::Leaf(_)), None) => {
                return Err(anyhow!("Unexpected file parent for a directory {:?}", path,));
            }
            (Some(Entry::Tree(_)), None) | (None, None) => {
                // Simple cases, nothing to do here
            }
        };

        let mut entry_stack = EntryStack {
            values: vec![],
            parent: parent.clone(),
        };
        let mut parent = parent.and_then(|e| e.into_tree());

        for cs_id in stack_of_commits.iter() {
            let delta = match delta_sub_entries.remove(cs_id) {
                Some(delta) => delta,
                None => {
                    // This directory hasn't been changed in `cs_id`, just continue...
                    if path.is_none() && cur_sub_entries.is_empty() && parent.is_none() {
                        // ... unless it's an empty root tree with no parents.
                        // That means we have an empty commit with no parents,
                        // and for that case let's create a new root object to match what
                        // derive_manifest() function is doing.
                        let (ctx, tree_id) = create_tree(
                            TreeInfo {
                                path: path.clone(),
                                parents: parent.clone().into_iter().collect(),
                                subentries: Default::default(),
                            },
                            *cs_id,
                        )
                        .await?;
                        parent = Some(tree_id.clone());
                        entry_stack
                            .values
                            .push((*cs_id, Some(ctx), Some(Entry::Tree(tree_id))));
                    }
                    continue;
                }
            };

            // Let's apply delta to get the subentries for a given commit
            for (path_elem, (ctx, maybe_entry)) in delta {
                match maybe_entry {
                    Some(entry) => {
                        cur_sub_entries.insert(path_elem, (ctx, entry));
                    }
                    None => {
                        cur_sub_entries.remove(&path_elem);
                    }
                }
            }

            // Finally let's create or delete the directory
            if !cur_sub_entries.is_empty() {
                let (ctx, tree_id) = create_tree(
                    TreeInfo {
                        path: path.clone(),
                        parents: parent.clone().into_iter().collect(),
                        subentries: cur_sub_entries.clone(),
                    },
                    *cs_id,
                )
                .await?;

                parent = Some(tree_id.clone());
                entry_stack
                    .values
                    .push((*cs_id, Some(ctx), Some(Entry::Tree(tree_id))));
            } else if path.is_none() {
                // Everything is deleted in the repo - let's create a new root
                // object
                let (ctx, tree_id) = create_tree(
                    TreeInfo {
                        path: path.clone(),
                        parents: parent.clone().into_iter().collect(),
                        subentries: Default::default(),
                    },
                    *cs_id,
                )
                .await?;
                parent = Some(tree_id.clone());
                entry_stack
                    .values
                    .push((*cs_id, Some(ctx), Some(Entry::Tree(tree_id))));
            } else {
                parent = None;
                entry_stack.values.push((*cs_id, None, None));
            }
        }

        Ok(entry_stack)
    }
}

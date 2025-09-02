/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::marker::PhantomData;

use anyhow::Error;
use anyhow::anyhow;
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures::future;
use futures::future::BoxFuture;
use futures::pin_mut;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::Stream;
use futures_watchdog::WatchdogExt;
use mononoke_macros::mononoke;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;
use mononoke_types::path::MPath;

use crate::Entry;
use crate::Manifest;
use crate::PathOrPrefix;
use crate::PathTree;
use crate::StoreLoadable;
use crate::select::select_path_tree;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diff<Entry> {
    Added(MPath, Entry),
    Removed(MPath, Entry),
    Changed(MPath, Entry, Entry),
}

impl<Entry> Diff<Entry> {
    pub fn replace_left(self, new_entry: Entry) -> Diff<Entry> {
        match self {
            Diff::Added(path, entry) => Diff::Changed(path, new_entry, entry),
            Diff::Removed(path, _) => Diff::Removed(path, new_entry),
            Diff::Changed(path, _, entry) => Diff::Changed(path, new_entry, entry),
        }
    }

    pub fn path(&self) -> &MPath {
        match self {
            Diff::Added(path, _) => path,
            Diff::Removed(path, _) => path,
            Diff::Changed(path, _, _) => path,
        }
    }
}

pub trait ManifestOps<Store>
where
    Store: Sync + Send + Clone + 'static,
    Self: StoreLoadable<Store> + Clone + Send + Sync + Eq + Unpin + 'static,
    <Self as StoreLoadable<Store>>::Value: Manifest<Store, TreeId = Self> + Send + Sync,
    <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf: Clone + Send + Eq + Unpin,
{
    fn find_entries_filtered<I, P, F>(
        &self,
        ctx: CoreContext,
        store: Store,
        paths_or_prefixes: I,
        filter: F,
    ) -> BoxStream<
        'static,
        Result<
            (
                MPath,
                Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf>,
            ),
            Error,
        >,
    >
    where
        I: IntoIterator<Item = P>,
        PathOrPrefix: From<P>,
        F: Fn(&MPath, Self) -> bool + Clone + Send + Sync + 'static,
    {
        let selector = select_path_tree(paths_or_prefixes);

        let init = Some((self.clone(), selector, MPath::ROOT, false));
        (async_stream::stream! {
            let store = &store;
            borrowed!(ctx, store, filter);
            let s = bounded_traversal::bounded_traversal_stream(
                256,
                init,
                move |(manifest_id, selector, path, recursive)| {
                    let (select, subentries) = selector.deconstruct();
                    cloned!(ctx, store, filter);
                    async move {
                        if !filter(&path, manifest_id.clone()) {
                            return Ok((Vec::new(), Vec::new()));
                        }
                        mononoke::spawn_task(async move {
                            let manifest = manifest_id.load(&ctx, &store).await?;
                            let mut output = Vec::new();
                            let mut recurse = Vec::new();
                            if recursive || select.is_recursive() {
                                output.push((path.clone(), Entry::Tree(manifest_id)));
                                let mut stream = manifest.list(&ctx, &store).await?;
                                while let Some((name, entry)) = stream.try_next().await? {
                                    let path = path.join(&name);
                                    match entry {
                                        Entry::Leaf(_) => {
                                            output.push((path.clone(), entry));
                                        }
                                        Entry::Tree(manifest_id) => {
                                            recurse.push((manifest_id, Default::default(), path, true));
                                        }
                                    }
                                }
                            } else {
                                if select.is_selected() {
                                    output.push((path.clone(), Entry::Tree(manifest_id)));
                                }
                                for (name, selector) in subentries {
                                    if let Some(entry) = manifest.lookup(&ctx, &store, &name).await? {
                                        let path = path.join(&name);
                                        match entry {
                                            Entry::Leaf(_) => {
                                                if selector.value.is_selected() {
                                                    output.push((path.clone(), entry));
                                                }
                                            }
                                            Entry::Tree(manifest_id) => {
                                                recurse.push((manifest_id, selector, path, false));
                                            }
                                        }
                                    }
                                }
                            }

                            Ok::<_, Error>((output, recurse))
                        }).await?
                    }.boxed()
                },
            )
            .map_ok(|entries| stream::iter(entries.into_iter().map(Ok)))
            .try_flatten();

            pin_mut!(s);
            while let Some(value) = s.next().await {
                yield value;
            }
        })
        .boxed()
    }

    fn find_entries<I, P>(
        &self,
        ctx: CoreContext,
        store: Store,
        paths_or_prefixes: I,
    ) -> BoxStream<
        'static,
        Result<
            (
                MPath,
                Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf>,
            ),
            Error,
        >,
    >
    where
        I: IntoIterator<Item = P>,
        PathOrPrefix: From<P>,
    {
        self.find_entries_filtered(ctx, store, paths_or_prefixes, |_, _| true)
    }

    fn find_entry(
        &self,
        ctx: CoreContext,
        store: Store,
        path: MPath,
    ) -> BoxFuture<
        'static,
        Result<
            Option<Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf>>,
            Error,
        >,
    > {
        self.find_entries(ctx, store, Some(PathOrPrefix::Path(path)))
            .into_future()
            .map(|(first, _rest)| Ok(first.transpose()?.map(|(_path, entry)| entry)))
            .boxed()
    }

    fn list_all_entries(
        &self,
        ctx: CoreContext,
        store: Store,
    ) -> BoxStream<
        'static,
        Result<
            (
                MPath,
                Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf>,
            ),
            Error,
        >,
    > {
        self.find_entries(ctx, store, vec![PathOrPrefix::Prefix(MPath::ROOT)])
    }

    fn list_leaf_entries(
        &self,
        ctx: CoreContext,
        store: Store,
    ) -> BoxStream<
        'static,
        Result<
            (
                NonRootMPath,
                <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf,
            ),
            Error,
        >,
    > {
        self.list_all_entries(ctx, store)
            .filter_map(|result| {
                let maybe_leaf = match result {
                    Ok((path, Entry::Leaf(filenode_id))) => match NonRootMPath::try_from(path) {
                        Ok(path) => Some(Ok((path, filenode_id))),
                        Err(e) => Some(Err(e)),
                    },
                    Err(err) => Some(Err(err)),
                    _ => None,
                };
                future::ready(maybe_leaf)
            })
            .boxed()
    }

    fn list_leaf_entries_under(
        &self,
        ctx: CoreContext,
        store: Store,
        prefixes: impl IntoIterator<Item = NonRootMPath>,
    ) -> BoxStream<
        'static,
        Result<
            (
                NonRootMPath,
                <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf,
            ),
            Error,
        >,
    > {
        self.find_entries(
            ctx,
            store,
            prefixes
                .into_iter()
                .map(|pref| PathOrPrefix::Prefix(pref.into())),
        )
        .filter_map(|result| {
            let maybe_leaf = match result {
                Ok((path, Entry::Leaf(filenode_id))) => match NonRootMPath::try_from(path) {
                    Ok(path) => Some(Ok((path, filenode_id))),
                    Err(e) => Some(Err(e)),
                },
                Err(err) => Some(Err(err)),
                _ => None,
            };
            future::ready(maybe_leaf)
        })
        .boxed()
    }

    fn list_tree_entries(
        &self,
        ctx: CoreContext,
        store: Store,
    ) -> BoxStream<
        'static,
        Result<
            (
                MPath,
                <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::TreeId,
            ),
            Error,
        >,
    > {
        self.list_all_entries(ctx, store)
            .filter_map(|result| {
                let maybe_tree = match result {
                    Ok((path, Entry::Tree(tree_id))) => Some(Ok((path, tree_id))),
                    Err(err) => Some(Err(err)),
                    _ => None,
                };
                future::ready(maybe_tree)
            })
            .boxed()
    }

    /// Returns differences between two manifests.
    ///
    /// `self` is considered the "old" manifest (so entries missing there are "Added")
    /// `other` is considered the "new" manifest (so entries missing there are "Removed")
    fn diff(
        &self,
        ctx: CoreContext,
        store: Store,
        other: Self,
    ) -> BoxStream<
        'static,
        Result<
            Diff<Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf>>,
            Error,
        >,
    > {
        self.filtered_diff(
            ctx,
            store.clone(),
            other,
            store,
            Some,
            |_| true,
            Default::default(),
        )
    }

    /// Do a diff, but with knobs to filter_map output and prune some subtrees.
    /// `output_filter` let's us configure what will be returned from filtered_diff. it accepts
    /// every diff entry and returns Option<Out>, so it acts similar to filter_map() function
    /// recurse_pruner is a function that allows us to skip iterating over some subtrees
    fn filtered_diff<FilterMap, Out, RecursePruner>(
        &self,
        ctx: CoreContext,
        store: Store,
        other: Self,
        other_store: Store,
        output_filter: FilterMap,
        recurse_pruner: RecursePruner,
        manifest_replacements: HashMap<
            MPath,
            Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf>,
        >,
    ) -> BoxStream<'static, Result<Out, Error>>
    where
        FilterMap: Fn(
                Diff<Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf>>,
            ) -> Option<Out>
            + Clone
            + Send
            + 'static,
        RecursePruner: Fn(&Diff<Self>) -> bool + Clone + Send + 'static,
        Out: Send + 'static,
    {
        let (replacement, child_replacements) =
            ReplacementsHolder::new(manifest_replacements).deconstruct();
        let this = match replacement {
            None => self.clone(),
            Some(Entry::Tree(replacement)) => replacement,
            Some(Entry::Leaf(_)) => {
                return stream::once(async move {
                    Err(anyhow!(
                        "Manifest replacement at root which resolves to a leaf"
                    ))
                })
                .boxed();
            }
        };

        if this == other {
            return stream::empty().boxed();
        }

        let input = Diff::Changed(MPath::ROOT, this, other);

        bounded_traversal::bounded_traversal_stream(
            256,
            Some((input, child_replacements)),
            move |(input, mut replacements)| {
                cloned!(ctx, output_filter, recurse_pruner, store, other_store);
                async move {
                    borrowed!(ctx);
                    let mut output = OutputHolder::new(output_filter);
                    let mut recurse = RecurseHolder::new(recurse_pruner);

                    match input {
                        Diff::Changed(path, left, right) => {
                            let l = mononoke::spawn_task({
                                cloned!(ctx, left, store);
                                async move { left.load(&ctx, &store).watched(ctx.logger()).await }
                            });
                            let r = mononoke::spawn_task({
                                cloned!(ctx, right, other_store);
                                async move { right.load(&ctx, &other_store).watched(ctx.logger()).await }
                            });
                            let (left_mf, right_mf) = future::try_join(l, r).await?;
                            let (left_mf, right_mf) = (left_mf?, right_mf?);

                            let mut stream = left_mf.list(ctx, &store).await?;
                            while let Some((name, left)) = stream.try_next().await? {
                                tokio::task::consume_budget().await;

                                let path = path.join(&name);
                                let (replacement, child_replacements) = replacements.remove(&name).unwrap_or_default().deconstruct();
                                let left = replacement.unwrap_or(left);

                                if let Some(right) =
                                    right_mf.lookup(ctx, &other_store, &name).await?
                                {
                                    if left != right {
                                        match (left, right) {
                                            (left @ Entry::Leaf(_), right @ Entry::Leaf(_)) => {
                                                output.push(Diff::Changed(path, left, right));
                                            }
                                            (Entry::Tree(tree), right @ Entry::Leaf(_)) => {
                                                output.push(Diff::Added(path.clone(), right));
                                                recurse.push(Diff::Removed(path, tree), child_replacements);
                                            }
                                            (left @ Entry::Leaf(_), Entry::Tree(tree)) => {
                                                output.push(Diff::Removed(path.clone(), left));
                                                recurse.push(Diff::Added(path, tree), child_replacements);
                                            }
                                            (Entry::Tree(left), Entry::Tree(right)) => {
                                                recurse.push(Diff::Changed(path, left, right), child_replacements)
                                            }
                                        }
                                    }
                                } else {
                                    match left {
                                        Entry::Tree(tree) => {
                                            recurse.push(Diff::Removed(path, tree), child_replacements)
                                        }
                                        _ => output.push(Diff::Removed(path, left)),
                                    }
                                }
                            }

                            let mut stream = right_mf.list(ctx, &other_store).await?;
                            while let Some((name, right)) = stream.try_next().await? {
                                tokio::task::consume_budget().await;

                                if left_mf.lookup(ctx, &store, &name).await?.is_none() {
                                    let path = path.join(&name);
                                    let (replacement, child_replacements) = replacements.remove(&name).unwrap_or_default().deconstruct();
                                    match (replacement, right) {
                                        (None, Entry::Tree(tree)) => recurse.push(Diff::Added(path, tree), child_replacements),
                                        (None, right) => output.push(Diff::Added(path, right)),
                                        (Some(left @ Entry::Leaf(_)), right @ Entry::Leaf(_)) => {
                                            output.push(Diff::Changed(path, left, right));
                                        }
                                        (Some(Entry::Tree(tree)), right @ Entry::Leaf(_)) => {
                                            output.push(Diff::Added(path.clone(), right));
                                            recurse.push(Diff::Removed(path, tree), child_replacements);
                                        }
                                        (Some(left @ Entry::Leaf(_)), Entry::Tree(tree)) => {
                                            output.push(Diff::Removed(path.clone(), left));
                                            recurse.push(Diff::Added(path, tree), child_replacements);
                                        }
                                        (Some(Entry::Tree(left)), Entry::Tree(right)) => {
                                            recurse.push(Diff::Changed(path, left, right), child_replacements)
                                        }
                                    }
                                }
                            }
                            ReplacementsHolder::finalize(&path, replacements)?;
                            output.push(Diff::Changed(path, Entry::Tree(left), Entry::Tree(right)));
                            anyhow::Ok((output.into_output(), recurse.into_diffs()))
                        }
                        Diff::Added(path, tree) => {
                            let manifest = tree.load(ctx, &other_store).await?;
                            let mut stream = manifest.list(ctx, &other_store).await?;
                            while let Some((name, right)) = stream.try_next().await? {
                                tokio::task::consume_budget().await;

                                let path = path.join(&name);
                                let (replacement, child_replacements) = replacements.remove(&name).unwrap_or_default().deconstruct();
                                match (replacement, right) {
                                    (None, Entry::Tree(tree)) => recurse.push(Diff::Added(path, tree), child_replacements),
                                    (None, right) => output.push(Diff::Added(path, right)),
                                    (Some(left @ Entry::Leaf(_)), right @ Entry::Leaf(_)) => {
                                        output.push(Diff::Changed(path, left, right));
                                    }
                                    (Some(Entry::Tree(tree)), right @ Entry::Leaf(_)) => {
                                        output.push(Diff::Added(path.clone(), right));
                                        recurse.push(Diff::Removed(path, tree), child_replacements);
                                    }
                                    (Some(left @ Entry::Leaf(_)), Entry::Tree(tree)) => {
                                        output.push(Diff::Removed(path.clone(), left));
                                        recurse.push(Diff::Added(path, tree), child_replacements);
                                    }
                                    (Some(Entry::Tree(left)), Entry::Tree(right)) => {
                                        recurse.push(Diff::Changed(path, left, right), child_replacements)
                                    }
                                }
                            }
                            ReplacementsHolder::finalize(&path, replacements)?;
                            output.push(Diff::Added(path, Entry::Tree(tree)));
                            anyhow::Ok((output.into_output(), recurse.into_diffs()))
                        }
                        Diff::Removed(path, tree) => {
                            let manifest = tree.load(ctx, &store).await?;
                            let mut stream = manifest.list(ctx, &store).await?;
                            while let Some((name, entry)) = stream.try_next().await? {
                                tokio::task::consume_budget().await;

                                let path = path.join(&name);
                                let (replacement, child_replacements) = replacements.remove(&name).unwrap_or_default().deconstruct();
                                let entry = replacement.unwrap_or(entry);
                                match entry {
                                    Entry::Tree(tree) => recurse.push(Diff::Removed(path, tree), child_replacements),
                                    _ => output.push(Diff::Removed(path, entry)),
                                }
                            }
                            ReplacementsHolder::finalize(&path, replacements)?;
                            output.push(Diff::Removed(path, Entry::Tree(tree)));
                            anyhow::Ok((output.into_output(), recurse.into_diffs()))
                        }
                    }
                }
                .boxed()
            },
        )
        .map_ok(|entries| stream::iter(entries.into_iter().map(Ok)))
        .try_flatten()
        .boxed()
    }
}

// Stores output of diff_filtered_function() for a single iterator of bounded traversal.
// It's just a simple vector together with a function that converts the output
struct OutputHolder<Entry, FilterMap, Out> {
    output: Vec<Out>,
    filter_map: FilterMap,
    __phantom: PhantomData<Entry>,
}

impl<Entry, FilterMap, Out> OutputHolder<Entry, FilterMap, Out>
where
    FilterMap: Fn(Diff<Entry>) -> Option<Out>,
{
    fn new(filter_map: FilterMap) -> Self {
        Self {
            output: vec![],
            filter_map,
            __phantom: PhantomData,
        }
    }

    fn push(&mut self, diff: Diff<Entry>) {
        self.output.extend((self.filter_map)(diff));
    }

    fn into_output(self) -> Vec<Out> {
        self.output
    }
}

// Stores bounded traversal recursion
// It's just a simple vector with a filter function
struct RecurseHolder<Entry, Pruner, Replacements> {
    diffs: Vec<(Diff<Entry>, Replacements)>,
    pruner: Pruner,
}

impl<Entry, Pruner, Replacements> RecurseHolder<Entry, Pruner, Replacements>
where
    Pruner: Fn(&Diff<Entry>) -> bool,
{
    fn new(pruner: Pruner) -> Self {
        Self {
            diffs: vec![],
            pruner,
        }
    }

    fn push(&mut self, diff: Diff<Entry>, replacements: Replacements) {
        if (self.pruner)(&diff) {
            self.diffs.push((diff, replacements));
        }
    }

    fn into_diffs(self) -> Vec<(Diff<Entry>, Replacements)> {
        self.diffs
    }
}

pub(crate) struct ReplacementsHolder<Entry> {
    replacements: PathTree<Option<Entry>>,
}

impl<Entry> ReplacementsHolder<Entry> {
    /// Create a new replacements holder for manifest entry replacements.  These entries will replace the entries at the given paths.
    pub fn new(replacements: HashMap<MPath, Entry>) -> Self {
        Self {
            replacements: replacements
                .into_iter()
                .map(|(path, entry)| (path, Some(entry)))
                .collect(),
        }
    }

    /// Deconstruct one level of replacements, returning the replacement entry at the current level (if any), and a collection of child replacement holders.
    pub fn deconstruct(self) -> (Option<Entry>, BTreeMap<MPathElement, Self>) {
        let (replacement, child_replacements) = self.replacements.deconstruct();
        let child_replacements: BTreeMap<_, _> = child_replacements
            .into_iter()
            .map(|(elem, replacements)| (elem, Self { replacements }))
            .collect();
        (replacement, child_replacements)
    }

    /// Complete processing of a collection of ReplacementsHolders, ensuring that all values have been consumed.
    pub fn finalize(
        path: &MPath,
        mut replacements: BTreeMap<MPathElement, Self>,
    ) -> Result<(), Error> {
        if let Some((name, _replacement)) = replacements.pop_first() {
            let path = path.join(&name);
            return Err(anyhow!(
                "Manifest replacement at {path} which doesn't exist in the comparison manifest"
            ));
        }
        Ok(())
    }
}

impl<Entry> Default for ReplacementsHolder<Entry> {
    fn default() -> Self {
        Self {
            replacements: PathTree::default(),
        }
    }
}

/// Finds subentries in mf_id manifest that are different from entries with the same name in
/// every manifest in `diff_against`. Note that removed entries ARE NOT INCLUDED!
/// F. e. if file 'A' hash HASH_1 in mf_if, HASH_2 and HASH_3 in diff_against, then it will
/// be returned. But if file 'A' has HASH_2 then it wont' be returned because it matches
/// HASH_2 in diff_against.
/// This implementation is more efficient for merges.
pub fn find_intersection_of_diffs<TreeId, Leaf, Store>(
    ctx: CoreContext,
    store: Store,
    mf_id: TreeId,
    diff_against: Vec<TreeId>,
) -> impl Stream<Item = Result<(MPath, Entry<TreeId, Leaf>), Error>> + 'static
where
    Store: Sync + Send + Clone + 'static,
    TreeId: StoreLoadable<Store> + Clone + Send + Sync + Eq + Unpin + 'static,
    <TreeId as StoreLoadable<Store>>::Value:
        Manifest<Store, TreeId = TreeId, Leaf = Leaf> + Send + Sync,
    Leaf: Clone + Send + Eq + Unpin + 'static,
{
    find_intersection_of_diffs_and_parents(ctx, store, mf_id, diff_against)
        .map_ok(|(path, entry, _)| (path, entry))
}

/// Like `find_intersection_of_diffs` but for each returned entry it also returns diff_against
/// entries with the same path.
pub fn find_intersection_of_diffs_and_parents<TreeId, Leaf, Store>(
    ctx: CoreContext,
    store: Store,
    mf_id: TreeId,
    diff_against: Vec<TreeId>,
) -> impl Stream<Item = Result<(MPath, Entry<TreeId, Leaf>, Vec<Entry<TreeId, Leaf>>), Error>> + 'static
where
    Store: Sync + Send + Clone + 'static,
    TreeId: StoreLoadable<Store> + Clone + Send + Sync + Eq + Unpin + 'static,
    <TreeId as StoreLoadable<Store>>::Value:
        Manifest<Store, TreeId = TreeId, Leaf = Leaf> + Send + Sync,
    Leaf: Clone + Send + Eq + Unpin + 'static,
{
    match diff_against.first().cloned() {
        Some(parent) => async move {
            mononoke::spawn_task(async move {
                let mut new_entries = Vec::new();
                let mut parent_diff = parent.diff(ctx.clone(), store.clone(), mf_id);
                while let Some(diff_entry) = parent_diff.try_next().await? {
                    match diff_entry {
                        Diff::Added(path, entry) => new_entries.push((path, entry, vec![])),
                        Diff::Removed(..) => continue,
                        Diff::Changed(path, parent_entry, entry) => {
                            new_entries.push((path, entry, vec![parent_entry]))
                        }
                    }
                }

                let paths: Vec<_> = new_entries
                    .clone()
                    .into_iter()
                    .map(|(path, _, _)| path)
                    .collect();

                let futs = diff_against.into_iter().skip(1).map(move |p| {
                    p.find_entries(ctx.clone(), store.clone(), paths.clone())
                        .try_collect::<HashMap<_, _>>()
                });
                let entries_in_parents = future::try_join_all(futs).await?;

                let mut res = vec![];
                for (path, unode, mut parent_entries) in new_entries {
                    let mut new_entry = true;
                    for p in &entries_in_parents {
                        if let Some(parent_entry) = p.get(&path) {
                            if parent_entry == &unode {
                                new_entry = false;
                                break;
                            } else {
                                parent_entries.push(parent_entry.clone());
                            }
                        }
                    }

                    if new_entry {
                        res.push((path, unode, parent_entries));
                    }
                }

                Ok(stream::iter(res.into_iter().map(Ok)))
            })
            .await?
        }
        .try_flatten_stream()
        .right_stream(),
        None => mf_id
            .list_all_entries(ctx, store)
            .map_ok(|(path, entry)| (path, entry, vec![]))
            .left_stream(),
    }
}

impl<TreeId, Store> ManifestOps<Store> for TreeId
where
    Store: Sync + Send + Clone + 'static,
    Self: StoreLoadable<Store> + Clone + Send + Sync + Eq + Unpin + 'static,
    <Self as StoreLoadable<Store>>::Value: Manifest<Store, TreeId = Self> + Send + Sync,
    <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf: Send + Clone + Eq + Unpin,
{
}

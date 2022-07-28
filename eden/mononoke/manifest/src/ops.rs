/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::select::select_path_tree;
use crate::AsyncManifest as Manifest;
use crate::Entry;
use crate::PathOrPrefix;
use crate::PathTree;
use crate::StoreLoadable;
use anyhow::Error;
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use futures::future;
use futures::future::BoxFuture;
use futures::pin_mut;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::Stream;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use mononoke_types::MPath;
use std::collections::HashMap;
use std::marker::PhantomData;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Diff<Entry> {
    Added(Option<MPath>, Entry),
    Removed(Option<MPath>, Entry),
    Changed(Option<MPath>, Entry, Entry),
}

pub trait ManifestOps<Store>
where
    Store: Sync + Send + Clone + 'static,
    Self: StoreLoadable<Store> + Clone + Send + Sync + Eq + Unpin + 'static,
    <Self as StoreLoadable<Store>>::Value: Manifest<Store, TreeId = Self> + Send + Sync,
    <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::LeafId: Clone + Send + Eq + Unpin,
{
    fn find_entries<I, P>(
        &self,
        ctx: CoreContext,
        store: Store,
        paths_or_prefixes: I,
    ) -> BoxStream<
        'static,
        Result<
            (
                Option<MPath>,
                Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::LeafId>,
            ),
            Error,
        >,
    >
    where
        I: IntoIterator<Item = P>,
        PathOrPrefix: From<P>,
    {
        let selector = select_path_tree(paths_or_prefixes);

        let init = Some((self.clone(), selector, None, false));
        (async_stream::stream! {
            let store = &store;
            borrowed!(ctx, store);
            let s = bounded_traversal::bounded_traversal_stream(
                256,
                init,
                move |(manifest_id, selector, path, recursive)| {
                    let PathTree {
                        subentries,
                        value: select,
                    } = selector;

                    async move {
                        let manifest = manifest_id.load(ctx, store).await?;

                        let mut output = Vec::new();
                        let mut recurse = Vec::new();

                        if recursive || select.is_recursive() {
                            output.push((path.clone(), Entry::Tree(manifest_id)));
                            let mut stream = manifest.list(ctx, store).await?;
                            while let Some((name, entry)) = stream.try_next().await? {
                                let path = Some(MPath::join_opt_element(path.as_ref(), &name));
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
                                if let Some(entry) = manifest.lookup(ctx, store, &name).await? {
                                    let path = Some(MPath::join_opt_element(path.as_ref(), &name));
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

    fn find_entry(
        &self,
        ctx: CoreContext,
        store: Store,
        path: Option<MPath>,
    ) -> BoxFuture<
        'static,
        Result<
            Option<Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::LeafId>>,
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
                Option<MPath>,
                Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::LeafId>,
            ),
            Error,
        >,
    > {
        self.find_entries(ctx, store, vec![PathOrPrefix::Prefix(None)])
    }

    fn list_leaf_entries(
        &self,
        ctx: CoreContext,
        store: Store,
    ) -> BoxStream<
        'static,
        Result<
            (
                MPath,
                <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::LeafId,
            ),
            Error,
        >,
    > {
        self.list_all_entries(ctx, store)
            .filter_map(|result| {
                let maybe_leaf = match result {
                    Ok((Some(path), Entry::Leaf(filenode_id))) => Some(Ok((path, filenode_id))),
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
        prefixes: impl IntoIterator<Item = MPath>,
    ) -> BoxStream<
        'static,
        Result<
            (
                MPath,
                <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::LeafId,
            ),
            Error,
        >,
    > {
        self.find_entries(
            ctx,
            store,
            prefixes
                .into_iter()
                .map(|pref| PathOrPrefix::Prefix(Some(pref))),
        )
        .filter_map(|result| {
            let maybe_leaf = match result {
                Ok((Some(path), Entry::Leaf(filenode_id))) => Some(Ok((path, filenode_id))),
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
                Option<MPath>,
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
            Diff<Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::LeafId>>,
            Error,
        >,
    > {
        self.filtered_diff(ctx, store.clone(), other, store, Some, |_| true)
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
    ) -> BoxStream<'static, Result<Out, Error>>
    where
        FilterMap: Fn(
                Diff<
                    Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::LeafId>,
                >,
            ) -> Option<Out>
            + Clone
            + Send
            + 'static,
        RecursePruner: Fn(&Diff<Self>) -> bool + Clone + Send + 'static,
        Out: Send + 'static,
    {
        if self == &other {
            return stream::empty().boxed();
        }

        bounded_traversal::bounded_traversal_stream(
            256,
            Some(Diff::Changed(None, self.clone(), other)),
            move |input| {
                cloned!(ctx, output_filter, recurse_pruner, store, other_store);
                async move {
                    borrowed!(ctx);
                    let mut output = OutputHolder::new(output_filter);
                    let mut recurse = RecurseHolder::new(recurse_pruner);

                    match input {
                        Diff::Changed(path, left, right) => {
                            let (left_mf, right_mf) = future::try_join(
                                left.load(ctx, &store),
                                right.load(ctx, &other_store),
                            )
                            .await?;

                            let mut stream = left_mf.list(ctx, &store).await?;
                            while let Some((name, left)) = stream.try_next().await? {
                                let path = Some(MPath::join_opt_element(path.as_ref(), &name));
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
                                                recurse.push(Diff::Removed(path, tree));
                                            }
                                            (left @ Entry::Leaf(_), Entry::Tree(tree)) => {
                                                output.push(Diff::Removed(path.clone(), left));
                                                recurse.push(Diff::Added(path, tree));
                                            }
                                            (Entry::Tree(left), Entry::Tree(right)) => {
                                                recurse.push(Diff::Changed(path, left, right))
                                            }
                                        }
                                    }
                                } else {
                                    match left {
                                        Entry::Tree(tree) => {
                                            recurse.push(Diff::Removed(path, tree))
                                        }
                                        _ => output.push(Diff::Removed(path, left)),
                                    }
                                }
                            }
                            let mut stream = right_mf.list(ctx, &other_store).await?;
                            while let Some((name, right)) = stream.try_next().await? {
                                if left_mf.lookup(ctx, &store, &name).await?.is_none() {
                                    let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                                    match right {
                                        Entry::Tree(tree) => recurse.push(Diff::Added(path, tree)),
                                        _ => output.push(Diff::Added(path, right)),
                                    }
                                }
                            }
                            output.push(Diff::Changed(path, Entry::Tree(left), Entry::Tree(right)));

                            Ok((output.into_output(), recurse.into_diffs()))
                        }
                        Diff::Added(path, tree) => {
                            let manifest = tree.load(ctx, &other_store).await?;
                            let mut stream = manifest.list(ctx, &other_store).await?;
                            while let Some((name, entry)) = stream.try_next().await? {
                                let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                                match entry {
                                    Entry::Tree(tree) => recurse.push(Diff::Added(path, tree)),
                                    _ => output.push(Diff::Added(path, entry)),
                                }
                            }
                            output.push(Diff::Added(path, Entry::Tree(tree)));
                            Ok((output.into_output(), recurse.into_diffs()))
                        }
                        Diff::Removed(path, tree) => {
                            let manifest = tree.load(ctx, &store).await?;
                            let mut stream = manifest.list(ctx, &store).await?;
                            while let Some((name, entry)) = stream.try_next().await? {
                                let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                                match entry {
                                    Entry::Tree(tree) => recurse.push(Diff::Removed(path, tree)),
                                    _ => output.push(Diff::Removed(path, entry)),
                                }
                            }
                            output.push(Diff::Removed(path, Entry::Tree(tree)));
                            Ok::<_, Error>((output.into_output(), recurse.into_diffs()))
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
struct RecurseHolder<Entry, Pruner> {
    diffs: Vec<Diff<Entry>>,
    pruner: Pruner,
}

impl<Entry, Pruner> RecurseHolder<Entry, Pruner>
where
    Pruner: Fn(&Diff<Entry>) -> bool,
{
    fn new(pruner: Pruner) -> Self {
        Self {
            diffs: vec![],
            pruner,
        }
    }

    fn push(&mut self, diff: Diff<Entry>) {
        if (self.pruner)(&diff) {
            self.diffs.push(diff);
        }
    }

    fn into_diffs(self) -> Vec<Diff<Entry>> {
        self.diffs
    }
}

/// Finds subentries in mf_id manifest that are different from entries with the same name in
/// every manifest in `diff_against`. Note that removed entries ARE NOT INCLUDED!
/// F. e. if file 'A' hash HASH_1 in mf_if, HASH_2 and HASH_3 in diff_against, then it will
/// be returned. But if file 'A' has HASH_2 then it wont' be returned because it matches
/// HASH_2 in diff_against.
/// This implementation is more efficient for merges.
pub fn find_intersection_of_diffs<TreeId, LeafId, Store>(
    ctx: CoreContext,
    store: Store,
    mf_id: TreeId,
    diff_against: Vec<TreeId>,
) -> impl Stream<Item = Result<(Option<MPath>, Entry<TreeId, LeafId>), Error>> + 'static
where
    Store: Sync + Send + Clone + 'static,
    TreeId: StoreLoadable<Store> + Clone + Send + Sync + Eq + Unpin + 'static,
    <TreeId as StoreLoadable<Store>>::Value:
        Manifest<Store, TreeId = TreeId, LeafId = LeafId> + Send + Sync,
    LeafId: Clone + Send + Eq + Unpin + 'static,
{
    find_intersection_of_diffs_and_parents(ctx, store, mf_id, diff_against)
        .map_ok(|(path, entry, _)| (path, entry))
}

/// Like `find_intersection_of_diffs` but for each returned entry it also returns diff_against
/// entries with the same path.
pub fn find_intersection_of_diffs_and_parents<TreeId, LeafId, Store>(
    ctx: CoreContext,
    store: Store,
    mf_id: TreeId,
    diff_against: Vec<TreeId>,
) -> impl Stream<
    Item = Result<
        (
            Option<MPath>,
            Entry<TreeId, LeafId>,
            Vec<Entry<TreeId, LeafId>>,
        ),
        Error,
    >,
> + 'static
where
    Store: Sync + Send + Clone + 'static,
    TreeId: StoreLoadable<Store> + Clone + Send + Sync + Eq + Unpin + 'static,
    <TreeId as StoreLoadable<Store>>::Value:
        Manifest<Store, TreeId = TreeId, LeafId = LeafId> + Send + Sync,
    LeafId: Clone + Send + Eq + Unpin + 'static,
{
    match diff_against.get(0).cloned() {
        Some(parent) => async move {
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
    <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::LeafId: Send + Clone + Eq + Unpin,
{
}

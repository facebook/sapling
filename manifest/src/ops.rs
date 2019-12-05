/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::{Entry, Manifest, PathTree, StoreLoadable};
use cloned::cloned;
use context::CoreContext;
use failure_ext::Error;
use futures::{future, stream, Future, Stream};
use futures_ext::{
    bounded_traversal::bounded_traversal_stream, BoxFuture, BoxStream, FutureExt, StreamExt,
};
use mononoke_types::MPath;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Diff<Entry> {
    Added(Option<MPath>, Entry),
    Removed(Option<MPath>, Entry),
    Changed(Option<MPath>, Entry, Entry),
}

#[derive(Debug, Clone)]
pub enum PathOrPrefix {
    Path(Option<MPath>),
    Prefix(Option<MPath>),
}

impl From<MPath> for PathOrPrefix {
    fn from(path: MPath) -> Self {
        PathOrPrefix::Path(Some(path))
    }
}

impl From<Option<MPath>> for PathOrPrefix {
    fn from(path: Option<MPath>) -> Self {
        PathOrPrefix::Path(path)
    }
}

pub trait ManifestOps<Store>
where
    Store: Sync + Send + Clone + 'static,
    Self: StoreLoadable<Store> + Clone + Send + Eq + 'static,
    <Self as StoreLoadable<Store>>::Value: Manifest<TreeId = Self> + Send,
    <<Self as StoreLoadable<Store>>::Value as Manifest>::LeafId: Clone + Send + Eq,
{
    fn find_entries<I, P>(
        &self,
        ctx: CoreContext,
        store: Store,
        paths_or_prefixes: I,
    ) -> BoxStream<
        (
            Option<MPath>,
            Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest>::LeafId>,
        ),
        Error,
    >
    where
        I: IntoIterator<Item = P>,
        PathOrPrefix: From<P>,
    {
        enum Select {
            Single,    // single entry selected
            Recursive, // whole subtree selected
            Skip,      // not selected
        }

        impl Select {
            fn is_selected(&self) -> bool {
                match self {
                    Select::Single | Select::Recursive => true,
                    Select::Skip => false,
                }
            }

            fn is_recursive(&self) -> bool {
                match self {
                    Select::Recursive => true,
                    _ => false,
                }
            }
        }

        impl Default for Select {
            fn default() -> Select {
                Select::Skip
            }
        }

        let selector: PathTree<Select> = paths_or_prefixes
            .into_iter()
            .map(|path_or_prefix| match PathOrPrefix::from(path_or_prefix) {
                PathOrPrefix::Path(path) => (path, Select::Single),
                PathOrPrefix::Prefix(path) => (path, Select::Recursive),
            })
            .collect();

        bounded_traversal_stream(
            256,
            Some((self.clone(), selector, None, false)),
            move |(manifest_id, selector, path, recursive)| {
                let PathTree {
                    subentries,
                    value: select,
                } = selector;

                manifest_id.load(ctx.clone(), &store).map(move |manifest| {
                    let mut output = Vec::new();
                    let mut recurse = Vec::new();

                    if recursive || select.is_recursive() {
                        output.push((path.clone(), Entry::Tree(manifest_id)));
                        for (name, entry) in manifest.list() {
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
                            if let Some(entry) = manifest.lookup(&name) {
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

                    (output, recurse)
                })
            },
        )
        .map(|entries| stream::iter_ok(entries))
        .flatten()
        .boxify()
    }

    fn find_entry(
        &self,
        ctx: CoreContext,
        store: Store,
        path: Option<MPath>,
    ) -> BoxFuture<
        Option<Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest>::LeafId>>,
        Error,
    > {
        self.find_entries(ctx, store, Some(PathOrPrefix::Path(path)))
            .into_future()
            .then(|result| match result {
                Ok((Some((_path, entry)), _stream)) => Ok(Some(entry)),
                Ok((None, _stream)) => Ok(None),
                Err((err, _stream)) => Err(err),
            })
            .boxify()
    }

    fn list_all_entries(
        &self,
        ctx: CoreContext,
        store: Store,
    ) -> BoxStream<
        (
            Option<MPath>,
            Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest>::LeafId>,
        ),
        Error,
    > {
        self.find_entries(ctx.clone(), store.clone(), vec![PathOrPrefix::Prefix(None)])
    }

    fn list_leaf_entries(
        &self,
        ctx: CoreContext,
        store: Store,
    ) -> BoxStream<
        (
            Option<MPath>,
            <<Self as StoreLoadable<Store>>::Value as Manifest>::LeafId,
        ),
        Error,
    > {
        self.list_all_entries(ctx, store)
            .filter_map(|(path, entry)| match entry {
                Entry::Leaf(filenode_id) => Some((path, filenode_id)),
                _ => None,
            })
            .boxify()
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
        Diff<Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest>::LeafId>>,
        Error,
    > {
        if self == &other {
            return stream::empty().boxify();
        }

        bounded_traversal_stream(
            256,
            Some(Diff::Changed(None, self.clone(), other)),
            move |input| match input {
                Diff::Changed(path, left, right) => left
                    .load(ctx.clone(), &store)
                    .join(right.load(ctx.clone(), &store))
                    .map(move |(left_mf, right_mf)| {
                        let mut output = Vec::new();
                        let mut recurse = Vec::new();

                        for (name, left) in left_mf.list() {
                            let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                            if let Some(right) = right_mf.lookup(&name) {
                                if left != right {
                                    match (left, right) {
                                        (left @ Entry::Leaf(_), right @ Entry::Leaf(_)) => {
                                            output.push(Diff::Changed(path, left, right))
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
                                    Entry::Tree(tree) => recurse.push(Diff::Removed(path, tree)),
                                    _ => output.push(Diff::Removed(path, left)),
                                }
                            }
                        }
                        for (name, right) in right_mf.list() {
                            if left_mf.lookup(&name).is_none() {
                                let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                                match right {
                                    Entry::Tree(tree) => recurse.push(Diff::Added(path, tree)),
                                    _ => output.push(Diff::Added(path, right)),
                                }
                            }
                        }
                        output.push(Diff::Changed(path, Entry::Tree(left), Entry::Tree(right)));

                        (output, recurse)
                    })
                    .left_future(),
                Diff::Added(path, tree) => {
                    tree.load(ctx.clone(), &store).map(move |manifest| {
                        let mut output = Vec::new();
                        let mut recurse = Vec::new();
                        for (name, entry) in manifest.list() {
                            let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                            match entry {
                                Entry::Tree(tree) => recurse.push(Diff::Added(path, tree)),
                                _ => output.push(Diff::Added(path, entry)),
                            }
                        }
                        output.push(Diff::Added(path, Entry::Tree(tree)));
                        (output, recurse)
                    })
                }
                .left_future()
                .right_future(),
                Diff::Removed(path, tree) => {
                    tree.load(ctx.clone(), &store).map(move |manifest| {
                        let mut output = Vec::new();
                        let mut recurse = Vec::new();
                        for (name, entry) in manifest.list() {
                            let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                            match entry {
                                Entry::Tree(tree) => recurse.push(Diff::Removed(path, tree)),
                                _ => output.push(Diff::Removed(path, entry)),
                            }
                        }
                        output.push(Diff::Removed(path, Entry::Tree(tree)));
                        (output, recurse)
                    })
                }
                .right_future()
                .right_future(),
            },
        )
        .map(|output| stream::iter_ok(output))
        .flatten()
        .boxify()
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
) -> impl Stream<Item = (Option<MPath>, Entry<TreeId, LeafId>), Error = Error>
where
    Store: Sync + Send + Clone + 'static,
    TreeId: StoreLoadable<Store> + Clone + Send + Eq + 'static,
    <TreeId as StoreLoadable<Store>>::Value: Manifest<TreeId = TreeId, LeafId = LeafId> + Send,
    LeafId: Clone + Send + Eq + 'static,
{
    match diff_against.get(0) {
        Some(parent) => (*parent)
            .diff(ctx.clone(), store.clone(), mf_id)
            .filter_map(|diff_entry| match diff_entry {
                Diff::Added(path, entry) => Some((path, entry)),
                Diff::Removed(..) => None,
                Diff::Changed(path, _, entry) => Some((path, entry)),
            })
            .collect()
            .and_then({
                cloned!(ctx);
                move |new_entries| {
                    let paths: Vec<_> = new_entries
                        .clone()
                        .into_iter()
                        .map(|(path, _)| path)
                        .collect();

                    let futs = diff_against.into_iter().skip(1).map(move |p| {
                        p.find_entries(ctx.clone(), store.clone(), paths.clone())
                            .collect_to::<HashMap<_, _>>()
                    });

                    future::join_all(futs).map(move |entries_in_parents| {
                        let mut res = vec![];

                        for (path, unode) in new_entries {
                            let mut new_entry = true;
                            for p in &entries_in_parents {
                                if p.get(&path) == Some(&unode) {
                                    new_entry = false;
                                    break;
                                }
                            }

                            if new_entry {
                                res.push((path, unode));
                            }
                        }

                        res
                    })
                }
            })
            .map(|entries| stream::iter_ok(entries))
            .flatten_stream()
            .left_stream(),
        None => mf_id
            .list_all_entries(ctx.clone(), store.clone())
            .right_stream(),
    }
}

impl<TreeId, Store> ManifestOps<Store> for TreeId
where
    Store: Sync + Send + Clone + 'static,
    Self: StoreLoadable<Store> + Clone + Send + Eq + 'static,
    <Self as StoreLoadable<Store>>::Value: Manifest<TreeId = Self> + Send,
    <<Self as StoreLoadable<Store>>::Value as Manifest>::LeafId: Send + Clone + Eq,
{
}

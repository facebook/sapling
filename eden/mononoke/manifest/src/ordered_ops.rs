/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::marker::Unpin;

use anyhow::Error;
use borrowed::borrowed;
use bounded_traversal::OrderedTraversal;
use context::CoreContext;
use futures::future::FutureExt;
use futures::pin_mut;
use futures::stream::{BoxStream, StreamExt};
use mononoke_types::{MPath, MPathElement};
use nonzero_ext::nonzero;

use crate::select::select_path_tree;
use crate::{Entry, Manifest, OrderedManifest, PathOrPrefix, PathTree, StoreLoadable};

/// Track where we are relative to the `after` parameter.
enum After {
    /// Include everything.
    All,

    /// Include all contents, but omit the directory itself.
    AllContents,

    /// Include everything in this directory after the named element and the
    /// subpath within that element.
    After(MPathElement, Option<MPath>),
}

impl After {
    fn new(mpath_opt: Option<&MPath>) -> Self {
        match mpath_opt {
            None => After::AllContents,
            Some(mpath) => {
                let (elem, rest) = mpath.split_first();
                After::After(elem.clone(), rest)
            }
        }
    }

    /// Returns true if this element should be skipped entirely.
    ///
    /// We don't skip entries that match exactly, even though they themselves
    /// will not be included.  If the element name matches then we still want
    /// to descend into subdirectories.
    fn skip(&self, name: &MPathElement) -> bool {
        match self {
            After::All | After::AllContents => false,
            After::After(elem, _) => name < elem,
        }
    }

    /// Returns true if this directory itself should be included.
    fn include_self(&self) -> bool {
        match self {
            After::All => true,
            After::AllContents | After::After(..) => false,
        }
    }

    /// Returns true if a file with the given name in this directory should be
    /// included.
    fn include_file(&self, name: &MPathElement) -> bool {
        match self {
            After::All | After::AllContents => true,
            After::After(elem, _) => name > elem,
        }
    }

    /// Enter a subdirectory.  The directory must be one that should be
    /// entered (i.e. skip is false).  Returns an instance of `After` suitable
    /// for the subdirectory.
    fn enter_dir(&self, name: &MPathElement) -> After {
        match self {
            After::All | After::AllContents => After::All,
            After::After(elem, rest) => {
                if name == elem {
                    After::new(rest.as_ref())
                } else {
                    debug_assert!(name > elem);
                    After::All
                }
            }
        }
    }
}

pub trait ManifestOrderedOps<Store>
where
    Store: Sync + Send + Clone + 'static,
    Self: StoreLoadable<Store> + Clone + Send + Sync + Eq + Unpin + 'static,
    <Self as StoreLoadable<Store>>::Value: Manifest<TreeId = Self> + OrderedManifest + Send,
    <<Self as StoreLoadable<Store>>::Value as Manifest>::LeafId: Clone + Send + Eq + Unpin,
{
    fn find_entries_ordered<I, P>(
        &self,
        ctx: CoreContext,
        store: Store,
        paths_or_prefixes: I,
        after: Option<Option<MPath>>,
    ) -> BoxStream<
        'static,
        Result<
            (
                Option<MPath>,
                Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest>::LeafId>,
            ),
            Error,
        >,
    >
    where
        I: IntoIterator<Item = P>,
        PathOrPrefix: From<P>,
    {
        let selector = select_path_tree(paths_or_prefixes);

        // Schedule a maximum of 256 concurrently unfolding directories.
        let schedule_max = nonzero!(256usize);

        // Allow queueing of up to 2,560 items, which would be 10 items per
        // directory at the maximum concurrency level.  Experiments show this
        // is a good balance of queueing items while not spending too long
        // determining what can be scheduled.
        let queue_max = nonzero!(2560usize);

        let after = match after {
            None => {
                // If `after` is `None`, then we include everything.
                After::All
            }
            Some(mpath_opt) => {
                // If `after` is `Some(None)`, then we include everything
                // after the root (i.e. not the root itself).
                After::new(mpath_opt.as_ref())
            }
        };

        let init = Some((
            queue_max.get(),
            (self.clone(), selector, None, false, after),
        ));
        (async_stream::stream! {
            let store = &store;
            borrowed!(ctx, store);
            let s = bounded_traversal::bounded_traversal_ordered_stream(
                schedule_max,
                queue_max,
                init,
                move |(manifest_id, selector, path, recursive, after)| {
                    let PathTree {
                        subentries,
                        value: select,
                    } = selector;

                    async move {
                        let manifest = manifest_id.load(ctx, &store).await?;

                        let mut output = Vec::new();

                        if recursive || select.is_recursive() {
                            if after.include_self() {
                                output.push(OrderedTraversal::Output((
                                    path.clone(),
                                    Entry::Tree(manifest_id),
                                )));
                            }
                            for (name, entry) in manifest.list_weighted() {
                                if after.skip(&name) {
                                    continue;
                                }
                                let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                                match entry {
                                    Entry::Leaf(leaf) => {
                                        if after.include_file(&name) {
                                            output.push(OrderedTraversal::Output((
                                                path.clone(),
                                                Entry::Leaf(leaf),
                                            )));
                                        }
                                    }
                                    Entry::Tree((weight, manifest_id)) => {
                                        output.push(OrderedTraversal::Recurse(
                                            weight,
                                            (
                                                manifest_id,
                                                Default::default(),
                                                path,
                                                true,
                                                after.enter_dir(&name),
                                            ),
                                        ));
                                    }
                                }
                            }
                        } else {
                            if after.include_self() && select.is_selected() {
                                output.push(OrderedTraversal::Output((
                                    path.clone(),
                                    Entry::Tree(manifest_id),
                                )));
                            }
                            for (name, selector) in subentries {
                                if after.skip(&name) {
                                    continue;
                                }
                                if let Some(entry) = manifest.lookup_weighted(&name) {
                                    let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                                    match entry {
                                        Entry::Leaf(leaf) => {
                                            if after.include_file(&name)
                                                && selector.value.is_selected()
                                            {
                                                output.push(OrderedTraversal::Output((
                                                    path.clone(),
                                                    Entry::Leaf(leaf),
                                                )));
                                            }
                                        }
                                        Entry::Tree((weight, manifest_id)) => {
                                            output.push(OrderedTraversal::Recurse(
                                                weight,
                                                (
                                                    manifest_id,
                                                    selector,
                                                    path,
                                                    false,
                                                    after.enter_dir(&name),
                                                ),
                                            ));
                                        }
                                    }
                                }
                            }
                        }

                        Ok::<_, Error>(output)
                    }
                    .boxed()
                },
            );

            pin_mut!(s);
            while let Some(value) = s.next().await {
                yield value;
            }
        })
        .boxed()
    }
}

impl<TreeId, Store> ManifestOrderedOps<Store> for TreeId
where
    Store: Sync + Send + Clone + 'static,
    Self: StoreLoadable<Store> + Clone + Send + Sync + Eq + Unpin + 'static,
    <Self as StoreLoadable<Store>>::Value: Manifest<TreeId = Self> + OrderedManifest + Send,
    <<Self as StoreLoadable<Store>>::Value as Manifest>::LeafId: Send + Clone + Eq + Unpin,
{
}

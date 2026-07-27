/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::iter::Peekable;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use blobstore::StoreLoadable;
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use futures::future;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_watchdog::WatchdogExt;
use mononoke_macros::mononoke;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use mononoke_types::MPathElementPrefix;
use mononoke_types::NonRootMPath;

use crate::Diff;
use crate::Entry;
use crate::Manifest;
use crate::OrderedManifest;
use crate::TrieMapOps;
use crate::ops::ReplacementsHolder;

/// How much of the trie keyspace a comparison result covers: a single complete
/// entry, or a whole unexpanded sub-trie under a byte-prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Span<EK, PK, TrieMapType, V> {
    /// A single resolved entry, identified by its complete key.
    Element(EK, V),
    /// A whole unexpanded sub-trie of entries sharing a byte-prefix.
    Prefix(PK, TrieMapType),
}

impl<EK, PK, T, V> Span<EK, PK, T, V> {
    /// Translate the keys of this span, leaving the trie/value payload untouched.
    fn map_keys<EK2, PK2>(
        self,
        fe: impl FnOnce(EK) -> EK2,
        fp: impl FnOnce(PK) -> PK2,
    ) -> Span<EK2, PK2, T, V> {
        match self {
            Span::Element(ek, v) => Span::Element(fe(ek), v),
            Span::Prefix(pk, t) => Span::Prefix(fp(pk), t),
        }
    }
}

/// Result of a multi-way comparison between a manifest tree and the merge of
/// a number of base manifest trees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Comparison<TrieMapType, V> {
    /// The span at this path is new.
    New(Span<NonRootMPath, (MPath, MPathElementPrefix), TrieMapType, V>),
    /// The entry at this path has changed compared to all of the bases.
    Changed(NonRootMPath, V, Vec<Option<V>>),
    /// The span at this path is the same as at least one of the bases (at the
    /// given index).
    Same(
        Span<NonRootMPath, (MPath, MPathElementPrefix), TrieMapType, V>,
        /// The index of the first base manifest that this span is the same as.
        usize,
    ),
    /// The span at this path has been removed.
    Removed(
        Span<NonRootMPath, (MPath, MPathElementPrefix), Vec<Option<TrieMapType>>, Vec<Option<V>>>,
    ),
}

/// Result of a multi-way comparison between a single manifest and the merge
/// of a number of base manifests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestComparison<TrieMapType, V> {
    /// The span at this path is new.
    New(Span<MPathElement, MPathElementPrefix, TrieMapType, V>),
    /// The entry at this path has changed compared to all of the bases.
    Changed(MPathElement, V, Vec<Option<V>>),
    /// The span at this path is the same as at least one of the bases (at the
    /// given index).
    Same(
        Span<MPathElement, MPathElementPrefix, TrieMapType, V>,
        /// The index of the first base manifest that this span is the same as.
        usize,
    ),
    /// The span at this path has been removed.
    Removed(Span<MPathElement, MPathElementPrefix, Vec<Option<TrieMapType>>, Vec<Option<V>>>),
}

pub async fn compare_manifest<'a, M, Store>(
    ctx: &'a CoreContext,
    blobstore: &'a Store,
    mf: M,
    base_mfs: Vec<Option<M>>,
) -> Result<
    impl Stream<Item = Result<ManifestComparison<M::TrieMapType, Entry<M::TreeId, M::Leaf>>>> + 'a,
>
where
    M: Manifest<Store>,
    M::TreeId: Send + Sync + Eq + 'static,
    M::Leaf: Send + Sync + Eq + 'static,
    M::TrieMapType: TrieMapOps<Store, Entry<M::TreeId, M::Leaf>> + Eq,
    Store: Send + Sync + 'static,
{
    compare_manifest_with_stores(ctx, blobstore, blobstore, mf, base_mfs).await
}

/// Like [`compare_manifest`], but `mf` and the base manifests may live in
/// different blobstores (e.g. cross-repo diffs). Subtree pruning compares
/// trie-map node ids, which are content-addressed and therefore valid across
/// blobstores; only diverging subtrees are expanded, each from its own store.
pub async fn compare_manifest_with_stores<'a, M, Store>(
    ctx: &'a CoreContext,
    mf_store: &'a Store,
    base_store: &'a Store,
    mf: M,
    base_mfs: Vec<Option<M>>,
) -> Result<
    impl Stream<Item = Result<ManifestComparison<M::TrieMapType, Entry<M::TreeId, M::Leaf>>>> + 'a,
>
where
    M: Manifest<Store>,
    M::TreeId: Send + Sync + Eq + 'static,
    M::Leaf: Send + Sync + Eq + 'static,
    M::TrieMapType: TrieMapOps<Store, Entry<M::TreeId, M::Leaf>> + Eq,
    Store: Send + Sync + 'static,
{
    let (mf_trie_map, base_mf_trie_maps) = future::try_join(
        mf.into_trie_map(ctx, mf_store),
        future::try_join_all(base_mfs.into_iter().map(|p| async move {
            match p {
                Some(p) => Ok(Some(p.into_trie_map(ctx, base_store).await?)),
                None => Ok(None),
            }
        })),
    )
    .await?;
    Ok(bounded_traversal::bounded_traversal_stream(
        256,
        Some((MPathElementPrefix::new(), mf_trie_map, base_mf_trie_maps)),
        {
            cloned!(ctx, mf_store, base_store);
            move |(prefix, mf_trie_map, base_mf_trie_maps)| {
                cloned!(ctx, mf_store, base_store);
                async move {
                    if let Some(index) = base_mf_trie_maps
                        .iter()
                        .position(|parent| parent.as_ref() == Some(&mf_trie_map))
                    {
                        return anyhow::Ok((
                            stream::iter(vec![Ok(ManifestComparison::Same(
                                Span::Prefix(prefix, mf_trie_map),
                                index,
                            ))]),
                            vec![],
                        ));
                    }

                    if base_mf_trie_maps.is_empty()
                        || base_mf_trie_maps
                            .iter()
                            .all(|parent| parent.as_ref().is_none_or(TrieMapOps::is_empty))
                    {
                        return Ok((
                            stream::iter(vec![Ok(ManifestComparison::New(Span::Prefix(
                                prefix,
                                mf_trie_map,
                            )))]),
                            vec![],
                        ));
                    }

                    borrowed!(ctx);
                    let ((mf_value, mf_children), expanded_base_mfs) = future::try_join(
                        mf_trie_map.expand(ctx, mf_store),
                        future::try_join_all(base_mf_trie_maps.into_iter().map({
                            |parent| async move {
                                match parent {
                                    Some(parent) => parent.expand(ctx, base_store).await,
                                    None => Ok((None, Vec::new())),
                                }
                            }
                        })),
                    )
                    .await?;
                    let (parent_values, parent_children): (Vec<_>, Vec<_>) =
                        expanded_base_mfs.into_iter().unzip();

                    let mut out = Vec::new();
                    let mut recurse = Vec::new();

                    if let Some(value) = mf_value {
                        if let Some(index) = parent_values
                            .iter()
                            .position(|parent_value| parent_value.as_ref() == Some(&value))
                        {
                            out.push(Ok(ManifestComparison::Same(
                                Span::Element(prefix.to_element()?, value),
                                index,
                            )));
                        } else if parent_values.is_empty()
                            || parent_values.iter().all(Option::is_none)
                        {
                            out.push(Ok(ManifestComparison::New(Span::Element(
                                prefix.to_element()?,
                                value,
                            ))));
                        } else {
                            out.push(Ok(ManifestComparison::Changed(
                                prefix.to_element()?,
                                value,
                                parent_values,
                            )));
                        }
                    } else if !parent_values.is_empty()
                        && !parent_values.iter().all(Option::is_none)
                    {
                        out.push(Ok(ManifestComparison::Removed(Span::Element(
                            prefix.to_element()?,
                            parent_values,
                        ))));
                    }

                    let mut diff_iter = DiffIter::new(mf_children, parent_children);

                    while let Some((ch, child_value, child_base_mfs)) = diff_iter.next() {
                        let mut prefix = prefix.clone();
                        prefix.push(ch)?;
                        if let Some(value) = child_value {
                            if let Some(index) = child_base_mfs
                                .iter()
                                .position(|parent| parent.as_ref() == Some(&value))
                            {
                                out.push(Ok(ManifestComparison::Same(
                                    Span::Prefix(prefix, value),
                                    index,
                                )));
                            } else if child_base_mfs.is_empty()
                                || child_base_mfs.iter().all(|mf| mf.is_none())
                            {
                                out.push(Ok(ManifestComparison::New(Span::Prefix(prefix, value))));
                            } else {
                                recurse.push((prefix, value, child_base_mfs));
                            }
                        } else if !child_base_mfs.is_empty()
                            && !child_base_mfs
                                .iter()
                                .all(|parent| parent.as_ref().is_none_or(TrieMapOps::is_empty))
                        {
                            out.push(Ok(ManifestComparison::Removed(Span::Prefix(
                                prefix,
                                child_base_mfs,
                            ))));
                        }
                    }

                    Ok((stream::iter(out), recurse))
                }
                .boxed()
            }
        },
    )
    .try_flatten())
}

struct DiffIter<TrieMapType> {
    mf: Peekable<<Vec<(u8, TrieMapType)> as std::iter::IntoIterator>::IntoIter>,
    base_mfs: Vec<Peekable<<Vec<(u8, TrieMapType)> as std::iter::IntoIterator>::IntoIter>>,
}

impl<TrieMapType> DiffIter<TrieMapType> {
    fn new(mf: Vec<(u8, TrieMapType)>, base_mfs: Vec<Vec<(u8, TrieMapType)>>) -> Self {
        Self {
            mf: mf.into_iter().peekable(),
            base_mfs: base_mfs
                .into_iter()
                .map(|p| p.into_iter().peekable())
                .collect(),
        }
    }

    fn next(&mut self) -> Option<(u8, Option<TrieMapType>, Vec<Option<TrieMapType>>)> {
        let mf_next_ch = self.mf.peek().map(|(k, _)| k).copied();
        let min_base_mfs_next_ch = self
            .base_mfs
            .iter_mut()
            .filter_map(|p| p.peek().map(|(k, _)| *k))
            .min();
        let next_ch = match (mf_next_ch, min_base_mfs_next_ch) {
            (None, None) => return None,
            (None, Some(ch)) => ch,
            (Some(ch), None) => ch,
            (Some(ch), Some(parent_ch)) => std::cmp::min(ch, parent_ch),
        };
        let next_mf = (Some(next_ch) == mf_next_ch)
            .then(|| self.mf.next().map(|(_, v)| v))
            .flatten();
        let next_base_mfs = self
            .base_mfs
            .iter_mut()
            .map(|p| {
                (p.peek().map(|(k, _)| *k) == Some(next_ch))
                    .then(|| p.next().map(|(_, v)| v))
                    .flatten()
            })
            .collect();
        Some((next_ch, next_mf, next_base_mfs))
    }
}

pub fn compare_manifest_tree<'a, M, Store>(
    ctx: &'a CoreContext,
    blobstore: &'a Store,
    manifest_id: M::TreeId,
    base_manifest_ids: Vec<M::TreeId>,
) -> impl Stream<Item = Result<Comparison<M::TrieMapType, Entry<M::TreeId, M::Leaf>>>> + 'a
where
    Store: Send + Sync + 'static,
    M: Manifest<Store> + Send + Sync + 'static,
    M::TreeId: StoreLoadable<Store, Value = M> + Clone + Send + Sync + Eq + 'static,
    M::Leaf: Send + Sync + Eq + 'static,
    M::TrieMapType: TrieMapOps<Store, Entry<M::TreeId, M::Leaf>> + Eq,
{
    let base_manifest_ids: Vec<_> = base_manifest_ids.into_iter().map(Some).collect();
    bounded_traversal::bounded_traversal_stream(
        256,
        Some((MPath::ROOT, manifest_id, base_manifest_ids)),
        {
            move |(path, manifest_id, base_manifest_ids)| {
                async move {
                    let (manifest, base_manifests) = future::try_join(
                        manifest_id.load(ctx, blobstore),
                        future::try_join_all(base_manifest_ids.iter().map(
                            |base_manifest_id| async move {
                                match base_manifest_id {
                                    Some(base_manifest_id) => {
                                        Ok(Some(base_manifest_id.load(ctx, blobstore).await?))
                                    }
                                    None => Ok(None),
                                }
                            },
                        )),
                    )
                    .await?;
                    let mut outs = Vec::new();
                    let mut recurse = Vec::new();
                    let mut cmps =
                        compare_manifest(ctx, blobstore, manifest, base_manifests).await?;
                    while let Some(cmp) = cmps.try_next().await? {
                        let to_tree_span = |span: Span<_, _, _, _>| {
                            span.map_keys(
                                |elem| path.join_into_non_root_mpath(&elem),
                                |prefix| (path.clone(), prefix),
                            )
                        };
                        outs.push(match cmp {
                            ManifestComparison::New(span) => Comparison::New(to_tree_span(span)),
                            ManifestComparison::Same(span, index) => {
                                Comparison::Same(to_tree_span(span), index)
                            }
                            ManifestComparison::Removed(span) => {
                                Comparison::Removed(span.map_keys(
                                    |elem| path.join_into_non_root_mpath(&elem),
                                    |prefix| (path.clone(), prefix),
                                ))
                            }
                            ManifestComparison::Changed(elem, entry, base_entries) => {
                                if let Entry::Tree(tree_id) = &entry {
                                    let base_tree_ids = base_entries
                                        .iter()
                                        .map(|base_entry| match base_entry {
                                            Some(Entry::Tree(tree_id)) => Some(tree_id.clone()),
                                            Some(Entry::Leaf(_)) | None => None,
                                        })
                                        .collect();
                                    recurse.push((
                                        path.join(&elem),
                                        tree_id.clone(),
                                        base_tree_ids,
                                    ));
                                }

                                Comparison::Changed(
                                    path.join_into_non_root_mpath(&elem),
                                    entry,
                                    base_entries,
                                )
                            }
                        });
                    }
                    anyhow::Ok((stream::iter(outs).map(Ok), recurse))
                }
                .boxed()
            }
        },
    )
    .try_flatten()
}

/// A queued subtree diff plus the manifest replacements that apply within it.
type RecurseWork<TreeId, Leaf> = (
    Diff<TreeId>,
    BTreeMap<MPathElement, ReplacementsHolder<Entry<TreeId, Leaf>>>,
);

/// Classify a single child, given its (optional) old and new entries, into an
/// optional leaf-level `Diff` output and an optional subtree `Diff` to recurse
/// into. Callers must have already established that the entries differ (equal
/// children produce no diff).
///
/// Generic over the tree-id payload `T` so both the unordered (`T = TreeId`) and
/// the ordered (`T = (Weight, TreeId)`) paths share the exact leaf/tree
/// classification; the ordered path strips the weight afterwards. A file<->dir
/// transition yields both a leaf output and a subtree recursion.
pub(crate) fn classify_child<T, Leaf>(
    path: MPath,
    old: Option<Entry<T, Leaf>>,
    new: Option<Entry<T, Leaf>>,
) -> (Option<Diff<Entry<T, Leaf>>>, Option<Diff<T>>) {
    match (old, new) {
        (Some(Entry::Leaf(old)), Some(Entry::Leaf(new))) => (
            Some(Diff::Changed(path, Entry::Leaf(old), Entry::Leaf(new))),
            None,
        ),
        (Some(Entry::Tree(old)), Some(new @ Entry::Leaf(_))) => (
            Some(Diff::Added(path.clone(), new)),
            Some(Diff::Removed(path, old)),
        ),
        (Some(old @ Entry::Leaf(_)), Some(Entry::Tree(new))) => (
            Some(Diff::Removed(path.clone(), old)),
            Some(Diff::Added(path, new)),
        ),
        (Some(Entry::Tree(old)), Some(Entry::Tree(new))) => {
            (None, Some(Diff::Changed(path, old, new)))
        }
        (Some(old @ Entry::Leaf(_)), None) => (Some(Diff::Removed(path, old)), None),
        (Some(Entry::Tree(old)), None) => (None, Some(Diff::Removed(path, old))),
        (None, Some(new @ Entry::Leaf(_))) => (Some(Diff::Added(path, new)), None),
        (None, Some(Entry::Tree(new))) => (None, Some(Diff::Added(path, new))),
        (None, None) => (None, None),
    }
}

/// Apply a [`classify_child`] result for the unordered paths: output the leaf
/// diff, and queue the subtree diff (carrying `replacements`) subject to the
/// pruner.
fn push_child<TreeId, Leaf, Pruner>(
    (output, recurse): (Option<Diff<Entry<TreeId, Leaf>>>, Option<Diff<TreeId>>),
    outs: &mut Vec<Diff<Entry<TreeId, Leaf>>>,
    recurse_work: &mut Vec<RecurseWork<TreeId, Leaf>>,
    replacements: BTreeMap<MPathElement, ReplacementsHolder<Entry<TreeId, Leaf>>>,
    recurse_pruner: &Pruner,
) where
    Pruner: Fn(&Diff<TreeId>) -> bool,
{
    if let Some(output) = output {
        outs.push(output);
    }
    if let Some(work) = recurse {
        if recurse_pruner(&work) {
            recurse_work.push((work, replacements));
        }
    }
}

/// Compare the children of two directory manifests, returning only the children
/// that differ as `(element, old_entry, new_entry)` tuples (`None` on a side
/// means the child is absent there). Identical sub-shards are pruned by their
/// content-addressed id -- never loaded -- via [`compare_manifest_with_stores`],
/// and byte-prefix spans of new/removed children are expanded to their
/// individual elements.
///
/// This is the shared node-level core behind both the unordered
/// [`diff_manifests`] and the ordered [`diff_weighted_children`]; the two differ
/// only in how they consume these per-child differences (recursive vs. weighted
/// ordered traversal). `old_mf` and `new_mf` may come from different blobstores.
///
/// The returned children are in unspecified order; callers that need them sorted
/// (e.g. the ordered path) must sort by element.
async fn diff_manifest_children<M, Store>(
    ctx: &CoreContext,
    new_store: &Store,
    new_mf: M,
    old_store: &Store,
    old_mf: M,
) -> Result<
    Vec<(
        MPathElement,
        Option<Entry<M::TreeId, M::Leaf>>,
        Option<Entry<M::TreeId, M::Leaf>>,
    )>,
>
where
    M: Manifest<Store>,
    M::TreeId: Send + Sync + Eq + 'static,
    M::Leaf: Send + Sync + Eq + 'static,
    M::TrieMapType: TrieMapOps<Store, Entry<M::TreeId, M::Leaf>> + Eq,
    Store: Send + Sync + 'static,
{
    let mut result = Vec::new();
    let mut cmps =
        compare_manifest_with_stores(ctx, new_store, old_store, new_mf, vec![Some(old_mf)]).await?;
    while let Some(cmp) = cmps.try_next().await? {
        match cmp {
            ManifestComparison::Same(..) => {}
            ManifestComparison::New(Span::Element(name, entry)) => {
                result.push((name, None, Some(entry)));
            }
            ManifestComparison::New(Span::Prefix(prefix, trie_map)) => {
                let mut entries = trie_map.into_stream(ctx, new_store).await?;
                while let Some((suffix, entry)) = entries.try_next().await? {
                    let name = prefix.clone().join_into_element(suffix)?;
                    result.push((name, None, Some(entry)));
                }
            }
            ManifestComparison::Removed(Span::Element(name, base_entries)) => {
                result.push((name, base_entries.into_iter().flatten().next(), None));
            }
            ManifestComparison::Removed(Span::Prefix(prefix, base_trie_maps)) => {
                if let Some(trie_map) = base_trie_maps.into_iter().flatten().next() {
                    let mut entries = trie_map.into_stream(ctx, old_store).await?;
                    while let Some((suffix, entry)) = entries.try_next().await? {
                        let name = prefix.clone().join_into_element(suffix)?;
                        result.push((name, Some(entry), None));
                    }
                }
            }
            ManifestComparison::Changed(name, new_entry, base_entries) => {
                result.push((
                    name,
                    base_entries.into_iter().flatten().next(),
                    Some(new_entry),
                ));
            }
        }
    }
    Ok(result)
}

/// Diff a single node by listing both sides and substituting old-side entries
/// with any `replacements`, returning the child `Diff`s plus the subtree work to
/// recurse into (with the pruner already applied, and the replacements that
/// apply within each subtree).
///
/// This is the list-based per-node core shared by [`diff_manifests`] (for
/// replacement-bearing subtrees) and [`crate::ManifestOps::filtered_diff_slow`]
/// (which stays generic over manifest types without `TrieMapOps`), so it must
/// not depend on `TrieMapOps`. `work` is the node to expand: `Changed` compares
/// both sides, `Added`/`Removed` enumerate one side (a replacement injects the
/// opposite side).
pub(crate) async fn diff_manifest_node_by_listing<TreeId, Leaf, Store, Pruner>(
    ctx: &CoreContext,
    old_store: &Store,
    new_store: &Store,
    work: Diff<TreeId>,
    mut replacements: BTreeMap<MPathElement, ReplacementsHolder<Entry<TreeId, Leaf>>>,
    recurse_pruner: Pruner,
) -> Result<(
    Vec<Diff<Entry<TreeId, Leaf>>>,
    Vec<RecurseWork<TreeId, Leaf>>,
)>
where
    Store: Clone + Send + Sync + 'static,
    TreeId: StoreLoadable<Store> + Clone + Send + Sync + Eq + Unpin + 'static,
    <TreeId as StoreLoadable<Store>>::Value:
        Manifest<Store, TreeId = TreeId, Leaf = Leaf> + Send + Sync,
    Leaf: Clone + Send + Eq + Unpin + 'static,
    // Owned (not `&Pruner`) so the future stays `Send` without requiring
    // `Pruner: Sync` -- `RecursePruner` on the diff APIs is only `Send`.
    Pruner: Fn(&Diff<TreeId>) -> bool + Send,
{
    let mut outs: Vec<Diff<Entry<TreeId, Leaf>>> = Vec::new();
    let mut recurse: Vec<RecurseWork<TreeId, Leaf>> = Vec::new();
    match work {
        Diff::Changed(path, left, right) => {
            let l = mononoke::spawn_task({
                cloned!(ctx, left, old_store);
                async move { left.load(&ctx, &old_store).watched().await }
            });
            let r = mononoke::spawn_task({
                cloned!(ctx, right, new_store);
                async move { right.load(&ctx, &new_store).watched().await }
            });
            let (left_mf, right_mf) = future::try_join(l, r).await?;
            let (left_mf, right_mf) = (left_mf?, right_mf?);

            let mut stream = left_mf.list(ctx, old_store).await?;
            while let Some((name, left)) = stream.try_next().await? {
                tokio::task::consume_budget().await;
                let child_path = path.join(&name);
                let (replacement, child_replacements) =
                    replacements.remove(&name).unwrap_or_default().deconstruct();
                let left = replacement.unwrap_or(left);
                let right = right_mf.lookup(ctx, new_store, &name).await?;
                if right.as_ref() != Some(&left) {
                    push_child(
                        classify_child(child_path, Some(left), right),
                        &mut outs,
                        &mut recurse,
                        child_replacements,
                        &recurse_pruner,
                    );
                }
            }

            let mut stream = right_mf.list(ctx, new_store).await?;
            while let Some((name, right)) = stream.try_next().await? {
                tokio::task::consume_budget().await;
                if left_mf.lookup(ctx, old_store, &name).await?.is_none() {
                    let child_path = path.join(&name);
                    let (replacement, child_replacements) =
                        replacements.remove(&name).unwrap_or_default().deconstruct();
                    push_child(
                        classify_child(child_path, replacement, Some(right)),
                        &mut outs,
                        &mut recurse,
                        child_replacements,
                        &recurse_pruner,
                    );
                }
            }
            ReplacementsHolder::finalize(&path, replacements)
                .context("Failed to finalize replacements for changed tree")?;
            outs.push(Diff::Changed(path, Entry::Tree(left), Entry::Tree(right)));
        }
        Diff::Added(path, tree) => {
            let manifest = tree.load(ctx, new_store).await?;
            let mut stream = manifest.list(ctx, new_store).await?;
            while let Some((name, right)) = stream.try_next().await? {
                tokio::task::consume_budget().await;
                let child_path = path.join(&name);
                let (replacement, child_replacements) =
                    replacements.remove(&name).unwrap_or_default().deconstruct();
                push_child(
                    classify_child(child_path, replacement, Some(right)),
                    &mut outs,
                    &mut recurse,
                    child_replacements,
                    &recurse_pruner,
                );
            }
            ReplacementsHolder::finalize(&path, replacements)
                .context("Failed to finalize replacements for added tree")?;
            outs.push(Diff::Added(path, Entry::Tree(tree)));
        }
        Diff::Removed(path, tree) => {
            let manifest = tree.load(ctx, old_store).await?;
            let mut stream = manifest.list(ctx, old_store).await?;
            while let Some((name, entry)) = stream.try_next().await? {
                tokio::task::consume_budget().await;
                let child_path = path.join(&name);
                let (replacement, child_replacements) =
                    replacements.remove(&name).unwrap_or_default().deconstruct();
                let entry = replacement.unwrap_or(entry);
                push_child(
                    classify_child(child_path, Some(entry), None),
                    &mut outs,
                    &mut recurse,
                    child_replacements,
                    &recurse_pruner,
                );
            }
            ReplacementsHolder::finalize(&path, replacements)
                .context("Failed to finalize replacements for removed tree")?;
            outs.push(Diff::Removed(path, Entry::Tree(tree)));
        }
    }
    Ok((outs, recurse))
}

/// A sharding-aware, two-store replacement for the recursion in
/// [`crate::ManifestOps::filtered_diff`].
///
/// Produces the same `Diff` entries as `old_id.filtered_diff(.., new_id, ..)`
/// with an identity output filter and the given `recurse_pruner`, but compares
/// each changed directory's children via [`compare_manifest_with_stores`]. For
/// sharded manifests (e.g. content manifests) this prunes identical sub-shards
/// by their content-addressed id WITHOUT loading them, instead of re-enumerating
/// the whole directory on both sides (which is what made content-manifest diffs
/// pathologically expensive -- see the `derived_data_use_content_manifests` SEV).
///
/// `old_id` (the "old"/base side) and `new_id` (the "new" side) may live in
/// different blobstores (cross-repo diffs); subtree-id pruning is valid across
/// blobstores because the ids are content hashes.
///
/// Supports `manifest_replacements` (like `filtered_diff`): subtrees carrying a
/// replacement are diffed by listing (the old-side entries are substituted),
/// while replacement-free subtrees keep the fast id-pruned path -- so a huge
/// directory off the replacement paths is still never enumerated.
pub(crate) fn diff_manifests<TreeId, Leaf, Store, Pruner>(
    ctx: CoreContext,
    old_store: Store,
    old_id: TreeId,
    new_store: Store,
    new_id: TreeId,
    recurse_pruner: Pruner,
    manifest_replacements: HashMap<MPath, Entry<TreeId, Leaf>>,
) -> impl Stream<Item = Result<Diff<Entry<TreeId, Leaf>>>>
where
    Store: Clone + Send + Sync + 'static,
    TreeId: StoreLoadable<Store> + Clone + Send + Sync + Eq + Unpin + 'static,
    <TreeId as StoreLoadable<Store>>::Value:
        Manifest<Store, TreeId = TreeId, Leaf = Leaf> + Send + Sync,
    Leaf: Clone + Send + Sync + Eq + Unpin + 'static,
    <<TreeId as StoreLoadable<Store>>::Value as Manifest<Store>>::TrieMapType:
        TrieMapOps<Store, Entry<TreeId, Leaf>> + Eq,
    Pruner: Fn(&Diff<TreeId>) -> bool + Clone + Send + 'static,
{
    let (root_replacement, child_replacements) =
        ReplacementsHolder::new(manifest_replacements).deconstruct();
    let old_id = match root_replacement {
        None => old_id,
        Some(Entry::Tree(replacement)) => replacement,
        Some(Entry::Leaf(_)) => {
            return stream::once(async move {
                Err::<Diff<Entry<TreeId, Leaf>>, _>(anyhow!(
                    "Manifest replacement at root which resolves to a leaf"
                ))
            })
            .boxed();
        }
    };

    let init = if old_id == new_id {
        None
    } else {
        Some((
            Diff::Changed(MPath::ROOT, old_id, new_id),
            child_replacements,
        ))
    };
    bounded_traversal::bounded_traversal_stream(256, init, move |(work, replacements)| {
        cloned!(ctx, old_store, new_store, recurse_pruner);
        async move {
            let ctx = &ctx;
            let old_store = &old_store;
            let new_store = &new_store;
            let (outs, recurse) = match work {
                // Fast path: a replacement-free subtree; compare children via
                // the shared comparison engine, pruning identical sub-shards by
                // id without loading them.
                Diff::Changed(path, old_id, new_id) if replacements.is_empty() => {
                    let (old_mf, new_mf) =
                        future::try_join(old_id.load(ctx, old_store), new_id.load(ctx, new_store))
                            .await?;
                    let mut outs: Vec<Diff<Entry<TreeId, Leaf>>> = Vec::new();
                    let mut recurse: Vec<RecurseWork<TreeId, Leaf>> = Vec::new();
                    for (name, old, new) in
                        diff_manifest_children(ctx, new_store, new_mf, old_store, old_mf).await?
                    {
                        let child_path = path.join(&name);
                        push_child(
                            classify_child(child_path, old, new),
                            &mut outs,
                            &mut recurse,
                            BTreeMap::new(),
                            &recurse_pruner,
                        );
                    }
                    outs.push(Diff::Changed(
                        path,
                        Entry::Tree(old_id),
                        Entry::Tree(new_id),
                    ));
                    (outs, recurse)
                }
                // A replacement applies somewhere in this subtree, or this is an
                // added/removed subtree: diff by listing. Replacement-free
                // children recurse back into the fast path above.
                work => {
                    diff_manifest_node_by_listing(
                        ctx,
                        old_store,
                        new_store,
                        work,
                        replacements,
                        recurse_pruner,
                    )
                    .await?
                }
            };
            anyhow::Ok((stream::iter(outs).map(Ok), recurse))
        }
        .boxed()
    })
    .try_flatten()
    .boxed()
}

/// Sharding-aware ordered child diff for a single directory level: returns the
/// children that differ between `old_id` and `new_id`, sorted by element name,
/// as weighted entries (matching `OrderedManifest::list_weighted`'s shape).
///
/// Used by the fast path of `filtered_diff_ordered`: identical sub-shards are
/// pruned by id (via `compare_manifest_with_stores`) instead of listing both
/// whole directories. Weights (which the ordered scheduler uses to bound its
/// queue) are fetched via `lookup_weighted` only for the differing *tree*
/// children -- those are few and are loaded during recursion anyway.
pub(crate) async fn diff_weighted_children<TreeId, Leaf, Store>(
    ctx: &CoreContext,
    old_store: &Store,
    old_id: &TreeId,
    new_store: &Store,
    new_id: &TreeId,
) -> Result<
    Vec<(
        MPathElement,
        Option<Entry<(crate::types::Weight, TreeId), Leaf>>,
        Option<Entry<(crate::types::Weight, TreeId), Leaf>>,
    )>,
>
where
    Store: Send + Sync + 'static,
    TreeId: StoreLoadable<Store> + Clone + Send + Sync + Eq + Unpin + 'static,
    <TreeId as StoreLoadable<Store>>::Value:
        OrderedManifest<Store> + Manifest<Store, TreeId = TreeId, Leaf = Leaf> + Send + Sync,
    Leaf: Clone + Send + Sync + Eq + Unpin + 'static,
    <<TreeId as StoreLoadable<Store>>::Value as Manifest<Store>>::TrieMapType:
        TrieMapOps<Store, Entry<TreeId, Leaf>> + Eq,
{
    let (old_mf, new_mf) =
        future::try_join(old_id.load(ctx, old_store), new_id.load(ctx, new_store)).await?;

    // id-pruned comparison of `new` vs `old` via the shared node-level core.
    let mut differing = diff_manifest_children(ctx, new_store, new_mf, old_store, old_mf).await?;
    if differing.is_empty() {
        return Ok(Vec::new());
    }
    // The ordered scheduler consumes children in element order.
    differing.sort_by(|(a, ..), (b, ..)| a.cmp(b));

    // Weights are only needed for tree entries. Reload the parents once if any
    // differing child is a tree, then resolve weights via `lookup_weighted`.
    let needs_weight = differing.iter().any(|(_, old_entry, new_entry)| {
        matches!(old_entry, Some(Entry::Tree(_))) || matches!(new_entry, Some(Entry::Tree(_)))
    });
    let (old_mf, new_mf) = if needs_weight {
        let (old_mf, new_mf) =
            future::try_join(old_id.load(ctx, old_store), new_id.load(ctx, new_store)).await?;
        (Some(old_mf), Some(new_mf))
    } else {
        (None, None)
    };

    let mut result = Vec::with_capacity(differing.len());
    for (name, old_entry, new_entry) in differing {
        let left = weight_entry(ctx, old_store, old_mf.as_ref(), &name, old_entry).await?;
        let right = weight_entry(ctx, new_store, new_mf.as_ref(), &name, new_entry).await?;
        result.push((name, left, right));
    }
    Ok(result)
}

/// Attach the rollup weight to a tree entry (leaves carry no weight), fetching
/// it from the parent manifest via `lookup_weighted`.
async fn weight_entry<TreeId, Leaf, Store, M>(
    ctx: &CoreContext,
    store: &Store,
    mf: Option<&M>,
    name: &MPathElement,
    entry: Option<Entry<TreeId, Leaf>>,
) -> Result<Option<Entry<(crate::types::Weight, TreeId), Leaf>>>
where
    Store: Send + Sync,
    M: OrderedManifest<Store, TreeId = TreeId, Leaf = Leaf> + Send + Sync,
    TreeId: Send + Sync,
    Leaf: Send + Sync,
{
    match entry {
        None => Ok(None),
        Some(Entry::Leaf(leaf)) => Ok(Some(Entry::Leaf(leaf))),
        Some(Entry::Tree(tree_id)) => {
            let weight = match mf
                .expect("needs_weight implies parents were loaded")
                .lookup_weighted(ctx, store, name)
                .await?
            {
                Some(Entry::Tree((weight, _))) => weight,
                _ => 0,
            };
            Ok(Some(Entry::Tree((weight, tree_id))))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use anyhow::anyhow;
    use blobstore::KeyedBlobstore;
    use blobstore::PutBehaviour;
    use borrowed::borrowed;
    use fbinit::FacebookInit;
    use futures::stream::TryStreamExt;
    use maplit::btreemap;
    use memblob::KeyedMemblob;
    use memblob::Memblob;
    use mononoke_macros::mononoke;
    use mononoke_types::FileType;
    use mononoke_types::SortedVectorTrieMap;
    use mononoke_types::path::MPath;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::ops::ManifestOps;
    // use crate::tests::test_manifest::TestLeaf;
    use crate::tests::test_manifest::TestLeafId;
    use crate::tests::test_manifest::TestManifestId;
    use crate::tests::test_manifest::derive_test_manifest;

    async fn get_trie_map(
        ctx: &CoreContext,
        blobstore: &Arc<dyn KeyedBlobstore>,
        mf: TestManifestId,
        path: &str,
        prefix: &str,
    ) -> Result<SortedVectorTrieMap<Entry<TestManifestId, (FileType, TestLeafId)>>> {
        let mf = mf
            .find_entry(ctx.clone(), blobstore.clone(), MPath::new(path)?)
            .await?
            .ok_or_else(|| anyhow!("path {path} not found"))?
            .into_tree()
            .ok_or_else(|| anyhow!("path {path} is not a tree"))?;
        let mut trie_map = mf
            .load(ctx, blobstore)
            .await?
            .into_trie_map(ctx, blobstore)
            .await?;
        for byte in prefix.as_bytes() {
            let (_, subentries) = trie_map.expand()?;
            let mut subentries = subentries.into_iter().collect::<BTreeMap<_, _>>();
            trie_map = subentries
                .remove(byte)
                .ok_or_else(|| anyhow!("prefix {prefix} not found at {path}"))?;
        }
        Ok(trie_map)
    }

    async fn get_entry(
        ctx: &CoreContext,
        blobstore: &Arc<dyn KeyedBlobstore>,
        mf: TestManifestId,
        path: &str,
    ) -> Result<Entry<TestManifestId, (FileType, TestLeafId)>> {
        mf.find_entry(ctx.clone(), blobstore.clone(), MPath::new(path)?)
            .await?
            .ok_or_else(|| anyhow!("path {path} not found"))
    }

    #[mononoke::fbinit_test]
    async fn test_compare_manifest_single_parent(fb: FacebookInit) -> Result<()> {
        let blobstore: Arc<dyn KeyedBlobstore> =
            Arc::new(KeyedMemblob::new(Memblob::new(PutBehaviour::Overwrite)));
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx, blobstore);

        let mf0 = derive_test_manifest(
            ctx,
            blobstore,
            vec![],
            btreemap! {
                "/dir1/file1" => Some("file1"),
                "/dir1/file2" =>  Some("file2"),
                "/dir2/file3" =>  Some("file3"),
                "/dir2/file4" =>  Some("file4"),
                "/dir2/dir3/file5" => Some("file5"),
                "/dir2/dir3/file6" =>  Some("file6"),
                "/dir4a/file7a" => Some("file7a"),
                "/dir4b/file7b" => Some("file7b"),
                "/file7" => Some("file7"),
                "/file8" => Some("file8"),
            },
        )
        .await?
        .unwrap();

        let mf1 = derive_test_manifest(
            ctx,
            blobstore,
            vec![mf0],
            btreemap! {
                "/dir1/file1" => Some("file1a"),
                "/dir2/file3" => None,
                "/dir2/file9" => Some("file9"),
                "/dir2/dir3/file5" => None,
                "/dir2/dir3/file6" => None,
                "/file7" => None,
                "/file7/file7" => Some("file7"),
            },
        )
        .await?
        .unwrap();

        let diff = compare_manifest(
            ctx,
            blobstore,
            mf1.load(ctx, blobstore).await?,
            vec![Some(mf0.load(ctx, blobstore).await?)],
        )
        .await?
        .try_collect::<Vec<_>>()
        .await?;

        assert_eq!(
            diff,
            vec![
                ManifestComparison::Same(
                    Span::Prefix(
                        MPathElementPrefix::from_slice(b"dir4")?,
                        get_trie_map(ctx, blobstore, mf1, "", "dir4").await?,
                    ),
                    0
                ),
                ManifestComparison::Same(
                    Span::Prefix(
                        MPathElementPrefix::from_slice(b"file8")?,
                        get_trie_map(ctx, blobstore, mf1, "", "file8").await?,
                    ),
                    0
                ),
                ManifestComparison::Changed(
                    MPathElement::new_from_slice(b"dir2")?,
                    get_entry(ctx, blobstore, mf1, "dir2").await?,
                    vec![Some(get_entry(ctx, blobstore, mf0, "dir2").await?)],
                ),
                ManifestComparison::Changed(
                    MPathElement::new_from_slice(b"dir1")?,
                    get_entry(ctx, blobstore, mf1, "dir1").await?,
                    vec![Some(get_entry(ctx, blobstore, mf0, "dir1").await?)],
                ),
                ManifestComparison::Changed(
                    MPathElement::new_from_slice(b"file7")?,
                    get_entry(ctx, blobstore, mf1, "file7").await?,
                    vec![Some(get_entry(ctx, blobstore, mf0, "file7").await?)],
                ),
            ]
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_compare_manifest_tree(fb: FacebookInit) -> Result<()> {
        let blobstore: Arc<dyn KeyedBlobstore> =
            Arc::new(KeyedMemblob::new(Memblob::new(PutBehaviour::Overwrite)));
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx, blobstore);

        let mf0 = derive_test_manifest(
            ctx,
            blobstore,
            vec![],
            btreemap! {
                "/dir1/file1" => Some("file1"),
                "/dir1/file2" =>  Some("file2"),
                "/dir2/file3" =>  Some("file3"),
                "/dir2/file4" =>  Some("file4"),
                "/dir2/dir3/file5" => Some("file5"),
                "/dir2/dir3/file6" =>  Some("file6"),
                "/dir4a/file7a" => Some("file7a"),
                "/dir5/file8" => Some("file8"),
                "/file7" => Some("file7"),
            },
        )
        .await?
        .unwrap();

        let mf1 = derive_test_manifest(
            ctx,
            blobstore,
            vec![mf0],
            btreemap! {
                "/dir1/file1" => Some("file1a"),
                "/dir2/file3" => None,
                "/dir2/file9" => Some("file9"),
                "/dir2/dir3/file5" => None,
                "/dir2/dir3/file6" => None,
                "/file7" => None,
                "/file7/file7" => Some("file7"),
            },
        )
        .await?
        .unwrap();

        let mf2 = derive_test_manifest(
            ctx,
            blobstore,
            vec![mf0],
            btreemap! {
                "/dir1/file1" => Some("file1b"),
                "/dir1/file1c" => Some("file1c"),
            },
        )
        .await?
        .unwrap();

        let mf3 = derive_test_manifest(
            ctx,
            blobstore,
            vec![mf1, mf2],
            btreemap! {
                "/dir1/file1" => Some("file1b"),
                "/dir1/file1c" => Some("file1c"),
                "/dir5/file8" => None,
                "/file7" => Some("file7"),
            },
        )
        .await?
        .unwrap();

        let diff1 = compare_manifest_tree::<crate::tests::test_manifest::TestManifest, _>(
            ctx,
            blobstore,
            mf1,
            vec![mf0],
        )
        .try_collect::<Vec<_>>()
        .await?;

        assert_eq!(
            diff1,
            vec![
                Comparison::Same(
                    Span::Prefix(
                        (MPath::ROOT, MPathElementPrefix::from_slice(b"dir4")?),
                        get_trie_map(ctx, blobstore, mf1, "", "dir4").await?,
                    ),
                    0
                ),
                Comparison::Same(
                    Span::Prefix(
                        (MPath::ROOT, MPathElementPrefix::from_slice(b"dir5")?),
                        get_trie_map(ctx, blobstore, mf1, "", "dir5").await?,
                    ),
                    0
                ),
                Comparison::Changed(
                    NonRootMPath::new("dir2")?,
                    get_entry(ctx, blobstore, mf1, "dir2").await?,
                    vec![Some(get_entry(ctx, blobstore, mf0, "dir2").await?)],
                ),
                Comparison::Changed(
                    NonRootMPath::new("dir1")?,
                    get_entry(ctx, blobstore, mf1, "dir1").await?,
                    vec![Some(get_entry(ctx, blobstore, mf0, "dir1").await?)],
                ),
                Comparison::Changed(
                    NonRootMPath::new("file7")?,
                    get_entry(ctx, blobstore, mf1, "file7").await?,
                    vec![Some(get_entry(ctx, blobstore, mf0, "file7").await?)],
                ),
                Comparison::New(Span::Prefix(
                    (MPath::new("file7")?, MPathElementPrefix::from_slice(b"")?),
                    get_trie_map(ctx, blobstore, mf1, "file7", "").await?,
                )),
                Comparison::Same(
                    Span::Prefix(
                        (
                            MPath::new("dir1")?,
                            MPathElementPrefix::from_slice(b"file2")?
                        ),
                        get_trie_map(ctx, blobstore, mf1, "dir1", "file2").await?,
                    ),
                    0
                ),
                Comparison::Changed(
                    NonRootMPath::new("dir1/file1")?,
                    get_entry(ctx, blobstore, mf1, "dir1/file1").await?,
                    vec![Some(get_entry(ctx, blobstore, mf0, "dir1/file1").await?,)],
                ),
                Comparison::Removed(Span::Prefix(
                    (MPath::new("dir2")?, MPathElementPrefix::from_slice(b"d")?),
                    vec![Some(get_trie_map(ctx, blobstore, mf0, "dir2", "d").await?)],
                )),
                Comparison::Removed(Span::Prefix(
                    (
                        MPath::new("dir2")?,
                        MPathElementPrefix::from_slice(b"file3")?
                    ),
                    vec![Some(
                        get_trie_map(ctx, blobstore, mf0, "dir2", "file3").await?
                    )],
                )),
                Comparison::Same(
                    Span::Prefix(
                        (
                            MPath::new("dir2")?,
                            MPathElementPrefix::from_slice(b"file4")?
                        ),
                        get_trie_map(ctx, blobstore, mf1, "dir2", "file4").await?,
                    ),
                    0
                ),
                Comparison::New(Span::Prefix(
                    (
                        MPath::new("dir2")?,
                        MPathElementPrefix::from_slice(b"file9")?
                    ),
                    get_trie_map(ctx, blobstore, mf1, "dir2", "file9").await?,
                )),
            ]
        );

        let diff2 = compare_manifest_tree::<crate::tests::test_manifest::TestManifest, _>(
            ctx,
            blobstore,
            mf2,
            vec![mf0],
        )
        .try_collect::<Vec<_>>()
        .await?;

        assert_eq!(
            diff2,
            vec![
                Comparison::Same(
                    Span::Prefix(
                        (MPath::ROOT, MPathElementPrefix::from_slice(b"f")?),
                        get_trie_map(ctx, blobstore, mf2, "", "f").await?,
                    ),
                    0
                ),
                Comparison::Same(
                    Span::Prefix(
                        (MPath::ROOT, MPathElementPrefix::from_slice(b"dir2")?),
                        get_trie_map(ctx, blobstore, mf2, "", "dir2").await?,
                    ),
                    0
                ),
                Comparison::Same(
                    Span::Prefix(
                        (MPath::ROOT, MPathElementPrefix::from_slice(b"dir4")?),
                        get_trie_map(ctx, blobstore, mf2, "", "dir4").await?,
                    ),
                    0
                ),
                Comparison::Same(
                    Span::Prefix(
                        (MPath::ROOT, MPathElementPrefix::from_slice(b"dir5")?),
                        get_trie_map(ctx, blobstore, mf2, "", "dir5").await?,
                    ),
                    0
                ),
                Comparison::Changed(
                    NonRootMPath::new("dir1")?,
                    get_entry(ctx, blobstore, mf2, "dir1").await?,
                    vec![Some(get_entry(ctx, blobstore, mf0, "dir1").await?)],
                ),
                Comparison::Same(
                    Span::Prefix(
                        (
                            MPath::new("dir1")?,
                            MPathElementPrefix::from_slice(b"file2")?
                        ),
                        get_trie_map(ctx, blobstore, mf2, "dir1", "file2").await?,
                    ),
                    0
                ),
                Comparison::Changed(
                    NonRootMPath::new("dir1/file1")?,
                    get_entry(ctx, blobstore, mf2, "dir1/file1").await?,
                    vec![Some(get_entry(ctx, blobstore, mf0, "dir1/file1").await?)],
                ),
                Comparison::New(Span::Prefix(
                    (
                        MPath::new("dir1")?,
                        MPathElementPrefix::from_slice(b"file1c")?
                    ),
                    get_trie_map(ctx, blobstore, mf2, "dir1", "file1c").await?,
                )),
            ]
        );

        let diff3 = compare_manifest_tree::<crate::tests::test_manifest::TestManifest, _>(
            ctx,
            blobstore,
            mf3,
            vec![mf1, mf2],
        )
        .try_collect::<Vec<_>>()
        .await?;

        assert_eq!(
            diff3,
            vec![
                Comparison::Same(
                    Span::Prefix(
                        (MPath::ROOT, MPathElementPrefix::from_slice(b"f")?),
                        get_trie_map(ctx, blobstore, mf3, "", "f").await?,
                    ),
                    1
                ),
                Comparison::Same(
                    Span::Prefix(
                        (MPath::ROOT, MPathElementPrefix::from_slice(b"dir1")?),
                        get_trie_map(ctx, blobstore, mf3, "", "dir1").await?,
                    ),
                    1
                ),
                Comparison::Same(
                    Span::Prefix(
                        (MPath::ROOT, MPathElementPrefix::from_slice(b"dir4")?),
                        get_trie_map(ctx, blobstore, mf3, "", "dir4").await?,
                    ),
                    0
                ),
                Comparison::Removed(Span::Prefix(
                    (MPath::ROOT, MPathElementPrefix::from_slice(b"dir5")?),
                    vec![
                        Some(get_trie_map(ctx, blobstore, mf1, "", "dir5").await?),
                        Some(get_trie_map(ctx, blobstore, mf2, "", "dir5").await?),
                    ],
                )),
                Comparison::Changed(
                    NonRootMPath::new("dir2")?,
                    get_entry(ctx, blobstore, mf3, "dir2").await?,
                    vec![
                        Some(get_entry(ctx, blobstore, mf1, "dir2").await?),
                        Some(get_entry(ctx, blobstore, mf2, "dir2").await?)
                    ],
                ),
                Comparison::Same(
                    Span::Prefix(
                        (MPath::new("dir2")?, MPathElementPrefix::from_slice(b"d")?),
                        get_trie_map(ctx, blobstore, mf3, "dir2", "d").await?,
                    ),
                    1
                ),
                Comparison::Same(
                    Span::Prefix(
                        (
                            MPath::new("dir2")?,
                            MPathElementPrefix::from_slice(b"file3")?
                        ),
                        get_trie_map(ctx, blobstore, mf3, "dir2", "file3").await?,
                    ),
                    1
                ),
                Comparison::Same(
                    Span::Prefix(
                        (
                            MPath::new("dir2")?,
                            MPathElementPrefix::from_slice(b"file4")?
                        ),
                        get_trie_map(ctx, blobstore, mf3, "dir2", "file4").await?,
                    ),
                    0
                ),
                Comparison::Same(
                    Span::Prefix(
                        (
                            MPath::new("dir2")?,
                            MPathElementPrefix::from_slice(b"file9")?
                        ),
                        get_trie_map(ctx, blobstore, mf3, "dir2", "file9").await?,
                    ),
                    0
                ),
            ]
        );

        Ok(())
    }
}

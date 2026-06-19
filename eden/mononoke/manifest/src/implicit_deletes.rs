/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::hash::Hash;

use anyhow::Result;
use blobstore::StoreLoadable;
use cloned::cloned;
use context::CoreContext;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::future;
use futures::stream;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;

use crate::Entry;
use crate::Manifest;
use crate::ManifestOps;

/// Get implicit directory deletes from a single parent manifest,
/// caused by introducing new paths into a child.
///
/// These may happen when a path, occupied by a dir in the parent manifest
/// becomes occupied by a file in the child. In this case all files in the
/// replaced directory are implicitly deleted.
/// For example, consider the following manifest:
/// ```ignore
/// p1/
///   p2/   <- a dir
///     p3  <- a file
///     p4  <- a file
/// ```
/// Now a child adds the following file: `/p1/p2`
/// New manifest looks like:
/// ```ignore
/// p1/
///   p2    <- f file
/// ```
/// Dir `/p1/p2` was implicitly deleted (meaning files `/p1/p2/p3` and
/// `/p1/p2/p4` were implicitly deleted)
fn get_implicit_deletes_single_parent<ManifestId, Store, I, L>(
    ctx: CoreContext,
    store: Store,
    paths_added_in_a_child: I,
    parent: ManifestId,
) -> impl Stream<Item = Result<NonRootMPath>>
where
    ManifestId: Hash + Eq + StoreLoadable<Store> + Send + Sync + ManifestOps<Store> + 'static,
    Store: Send + Sync + Clone + 'static,
    <ManifestId as StoreLoadable<Store>>::Value:
        Manifest<Store, TreeId = ManifestId, Leaf = L> + Send + Sync,
    <<ManifestId as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf: Send + Copy + Eq,
    I: IntoIterator<Item = NonRootMPath>,
    L: Unpin,
{
    parent
        .find_entries(ctx.clone(), store.clone(), paths_added_in_a_child)
        .map({
            move |mb_entry| {
                match mb_entry {
                    Ok((path_added_in_a_child, entry)) => {
                        cloned!(ctx, store);

                        let path_added_in_a_child: NonRootMPath =
                            path_added_in_a_child.try_into()?;

                        match entry {
                            // No such path in parent or such path used to be a leaf.
                            // No implicit delete in both cases
                            Entry::Leaf(_) => anyhow::Ok(stream::empty().left_stream()),
                            Entry::Tree(tree) => anyhow::Ok(
                                tree.list_leaf_entries(ctx, store)
                                    .map_ok({
                                        cloned!(path_added_in_a_child);
                                        move |(relative_path, _)| {
                                            path_added_in_a_child.join(&relative_path)
                                        }
                                    })
                                    .right_stream(),
                            ),
                        }
                    }
                    Err(err) => Err(err),
                }
            }
        })
        .try_flatten_unordered(None)
}

/// Get implicit deletes in parent manifests,
/// caused by introducing new paths into a child
pub fn get_implicit_deletes<'a, ManifestId, Store, I, M, L>(
    ctx: &'a CoreContext,
    store: Store,
    paths_added_in_a_child: I,
    parents: M,
) -> impl Stream<Item = Result<NonRootMPath>> + 'a
where
    ManifestId: Hash + Eq + StoreLoadable<Store> + Send + Sync + ManifestOps<Store> + 'static,
    Store: Send + Sync + Clone + 'static,
    <ManifestId as StoreLoadable<Store>>::Value:
        Manifest<Store, TreeId = ManifestId, Leaf = L> + Send + Sync,
    <<ManifestId as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf: Send + Copy + Eq,
    I: IntoIterator<Item = NonRootMPath> + Clone + 'a,
    M: IntoIterator<Item = ManifestId> + 'a,
    L: Unpin,
{
    stream::iter(parents)
        .map(move |parent| {
            get_implicit_deletes_single_parent(
                ctx.clone(),
                store.clone(),
                paths_added_in_a_child.clone(),
                parent,
            )
        })
        // Setting this to a lower value because `get_implicit_deletes_single_parent`
        // already has a high degree of concurrency.
        .flatten_unordered(10)
        .try_filter({
            let mut seen = HashSet::new();
            move |item: &NonRootMPath| future::ready(seen.insert(item.clone()))
        })
}

/// The output of [`get_implicit_deletes_and_existing_files`].
pub struct ImplicitDeletesAndExistingFiles {
    /// See [`get_implicit_deletes`].
    pub implicit_deletes: Vec<NonRootMPath>,
    /// Input paths that are already a file in every parent.
    pub existing_files_in_all_parents: HashSet<NonRootMPath>,
}

/// Like [`get_implicit_deletes`], but also reports which input paths are already
/// a file in every parent. Both come from one `find_entries` per parent.
pub async fn get_implicit_deletes_and_existing_files<ManifestId, Store, I, M, L>(
    ctx: &CoreContext,
    store: Store,
    paths: I,
    parents: M,
) -> Result<ImplicitDeletesAndExistingFiles>
where
    ManifestId: Hash + Eq + StoreLoadable<Store> + Send + Sync + ManifestOps<Store> + 'static,
    Store: Send + Sync + Clone + 'static,
    <ManifestId as StoreLoadable<Store>>::Value:
        Manifest<Store, TreeId = ManifestId, Leaf = L> + Send + Sync,
    <<ManifestId as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf: Send + Copy + Eq,
    I: IntoIterator<Item = NonRootMPath> + Clone,
    M: IntoIterator<Item = ManifestId>,
    L: Unpin,
{
    let per_parent: Vec<(HashSet<NonRootMPath>, Vec<NonRootMPath>)> = stream::iter(parents)
        .map(|parent| {
            cloned!(ctx, store, paths);
            async move {
                let entries: Vec<(MPath, Entry<ManifestId, L>)> = parent
                    .find_entries(ctx.clone(), store.clone(), paths)
                    .try_collect()
                    .await?;

                // Leaf -> already a file; tree -> replaced dir whose files are deleted.
                let mut leaves = HashSet::new();
                let mut replaced_dirs = Vec::new();
                for (path, entry) in entries {
                    let path: NonRootMPath = path.try_into()?;
                    match entry {
                        Entry::Leaf(_) => {
                            leaves.insert(path);
                        }
                        Entry::Tree(tree_id) => replaced_dirs.push((path, tree_id)),
                    }
                }

                let implicit_deletes: Vec<NonRootMPath> = stream::iter(replaced_dirs)
                    .map(|(path, tree_id)| {
                        cloned!(ctx, store);
                        async move {
                            tree_id
                                .list_leaf_entries(ctx, store)
                                .map_ok(move |(relative_path, _)| path.join(&relative_path))
                                .try_collect::<Vec<_>>()
                                .await
                        }
                    })
                    .buffer_unordered(100)
                    .try_concat()
                    .await?;

                anyhow::Ok((leaves, implicit_deletes))
            }
        })
        .buffer_unordered(10)
        .try_collect()
        .await?;

    let (leaf_sets, implicit_delete_lists): (Vec<_>, Vec<_>) = per_parent.into_iter().unzip();

    // Dedup across parents.
    let implicit_deletes = implicit_delete_lists
        .into_iter()
        .flatten()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    // Leaf in every parent; empty for a root changeset (no parents).
    let existing_files_in_all_parents = leaf_sets
        .into_iter()
        .reduce(|acc, leaves| acc.intersection(&leaves).cloned().collect())
        .unwrap_or_default();

    Ok(ImplicitDeletesAndExistingFiles {
        implicit_deletes,
        existing_files_in_all_parents,
    })
}

#[cfg(test)]
mod test {
    use std::fmt::Debug;
    use std::sync::Arc;

    use blobstore::KeyedBlobstore;
    use blobstore::Storable;
    use fbinit::FacebookInit;
    use memblob::KeyedMemblob;
    use mononoke_macros::mononoke;
    use mononoke_types::FileType;

    use super::*;
    use crate::tests::test_manifest::TestLeaf;
    use crate::tests::test_manifest::TestManifest;
    use crate::tests::test_manifest::TestManifestId;

    fn ensure_unordered_eq<T: Debug + Hash + PartialEq + Eq, I: IntoIterator<Item = T>>(
        v1: I,
        v2: I,
    ) {
        let hs1: HashSet<_> = v1.into_iter().collect();
        let hs2: HashSet<_> = v2.into_iter().collect();
        assert_eq!(hs1, hs2);
    }

    #[mononoke::fbinit_test]
    async fn test_get_implicit_deletes_single_parent(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let store: Arc<dyn KeyedBlobstore> = Arc::new(KeyedMemblob::default());

        // Parent manifest looks like:
        // p1/
        //   p2/
        //     p3
        //     p4
        //   p5/
        //     p6
        let leaf6_id = TestLeaf::new("6").store(&ctx, &store).await?;
        let tree5_id = TestManifest::new()
            .insert("p6", Entry::Leaf((FileType::Regular, leaf6_id)))
            .store(&ctx, &store)
            .await?;
        let leaf4_id = TestLeaf::new("4").store(&ctx, &store).await?;
        let leaf3_id = TestLeaf::new("3").store(&ctx, &store).await?;
        let tree2_id = TestManifest::new()
            .insert("p3", Entry::Leaf((FileType::Regular, leaf3_id)))
            .insert("p4", Entry::Leaf((FileType::Regular, leaf4_id)))
            .store(&ctx, &store)
            .await?;
        let tree1_id = TestManifest::new()
            .insert("p2", Entry::Tree(tree2_id))
            .insert("p5", Entry::Tree(tree5_id))
            .store(&ctx, &store)
            .await?;
        let root_manifest = TestManifest::new()
            .insert("p1", Entry::Tree(tree1_id))
            .store(&ctx, &store)
            .await?;

        // Child adds a file at /p1/p2
        let implicitly_deleted_files: Vec<_> = get_implicit_deletes_single_parent(
            ctx.clone(),
            store.clone(),
            vec![NonRootMPath::new("p1/p2")?],
            root_manifest,
        )
        .try_collect()
        .await?;
        // We expect the entire /p1/p2 dir to be implicitly deleted
        // (and we enumerate all files: /p1/p2/p3 and /p1/p2/p4)
        ensure_unordered_eq(
            implicitly_deleted_files,
            vec![
                NonRootMPath::new("p1/p2/p3")?,
                NonRootMPath::new("p1/p2/p4")?,
            ],
        );

        // Adding an unrelated file should not cause any implicit deletes
        let implicitly_deleted_files: Vec<_> = get_implicit_deletes_single_parent(
            ctx.clone(),
            store.clone(),
            vec![NonRootMPath::new("p1/p200")?],
            root_manifest,
        )
        .try_collect()
        .await?;
        assert_eq!(implicitly_deleted_files, vec![]);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_implicit_deletes(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let store: Arc<dyn KeyedBlobstore> = Arc::new(KeyedMemblob::default());
        // Parent 1 manifest looks like:
        // p1/
        //   p2/
        //     p3
        //     p4
        //   p5/
        //     p6
        //   p7
        // p8
        let leaf8_id = TestLeaf::new("8").store(&ctx, &store).await?;
        let leaf7_id = TestLeaf::new("7").store(&ctx, &store).await?;
        let leaf6_id = TestLeaf::new("6").store(&ctx, &store).await?;
        let tree5_id = TestManifest::new()
            .insert("p6", Entry::Leaf((FileType::Regular, leaf6_id)))
            .store(&ctx, &store)
            .await?;
        let leaf4_id = TestLeaf::new("4").store(&ctx, &store).await?;
        let leaf3_id = TestLeaf::new("3").store(&ctx, &store).await?;
        let tree2_id = TestManifest::new()
            .insert("p3", Entry::Leaf((FileType::Regular, leaf3_id)))
            .insert("p4", Entry::Leaf((FileType::Regular, leaf4_id)))
            .store(&ctx, &store)
            .await?;
        let tree1_id = TestManifest::new()
            .insert("p2", Entry::Tree(tree2_id))
            .insert("p5", Entry::Tree(tree5_id))
            .insert("p7", Entry::Leaf((FileType::Regular, leaf7_id)))
            .store(&ctx, &store)
            .await?;
        let root_manifest_1 = TestManifest::new()
            .insert("p1", Entry::Tree(tree1_id))
            .insert("p8", Entry::Leaf((FileType::Regular, leaf8_id)))
            .store(&ctx, &store)
            .await?;

        // Parent 2 manifest looks like:
        // p1/
        //   p2/
        //     p3
        //     p4
        //   p5/
        //     p6
        //   p9/
        //     p10
        // p11
        let leaf11_id = TestLeaf::new("11").store(&ctx, &store).await?;
        let leaf10_id = TestLeaf::new("10").store(&ctx, &store).await?;
        let tree9_id = TestManifest::new()
            .insert("p10", Entry::Leaf((FileType::Executable, leaf10_id)))
            .store(&ctx, &store)
            .await?;
        let tree1b_id = TestManifest::new()
            .insert("p2", Entry::Tree(tree2_id))
            .insert("p5", Entry::Tree(tree5_id))
            .insert("p9", Entry::Tree(tree9_id))
            .store(&ctx, &store)
            .await?;
        let root_manifest_2 = TestManifest::new()
            .insert("p1", Entry::Tree(tree1b_id))
            .insert("p11", Entry::Leaf((FileType::Regular, leaf11_id)))
            .store(&ctx, &store)
            .await?;

        // Child adds files at /p1/p2, /p1/p7/p12 and /p1/p9
        let implicitly_deleted_files: Vec<_> = get_implicit_deletes(
            &ctx,
            store.clone(),
            vec![
                NonRootMPath::new("p1/p2")?,
                NonRootMPath::new("p1/p7/p12")?,
                NonRootMPath::new("p1/p9")?,
            ],
            vec![root_manifest_1, root_manifest_2],
        )
        .try_collect()
        .await?;

        ensure_unordered_eq(
            implicitly_deleted_files,
            vec![
                NonRootMPath::new("p1/p2/p3")?,
                NonRootMPath::new("p1/p2/p4")?,
                NonRootMPath::new("p1/p9/p10")?,
            ],
        );

        // Result should not depend on the order of parents
        let implicitly_deleted_files: Vec<_> = get_implicit_deletes(
            &ctx,
            store,
            vec![
                NonRootMPath::new("p1/p2")?,
                NonRootMPath::new("p1/p7/p12")?,
                NonRootMPath::new("p1/p9")?,
            ],
            vec![root_manifest_2, root_manifest_1],
        )
        .try_collect()
        .await?;

        ensure_unordered_eq(
            implicitly_deleted_files,
            vec![
                NonRootMPath::new("p1/p2/p3")?,
                NonRootMPath::new("p1/p2/p4")?,
                NonRootMPath::new("p1/p9/p10")?,
            ],
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_get_implicit_deletes_and_existing_files(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let store: Arc<dyn KeyedBlobstore> = Arc::new(KeyedMemblob::default());

        // Leaf content doesn't affect classification, so reuse one leaf id.
        let leaf_id = TestLeaf::new("x").store(&ctx, &store).await?;
        let leaf = || Entry::Leaf((FileType::Regular, leaf_id));

        let d_tree = TestManifest::new()
            .insert("a", leaf())
            .insert("b", leaf())
            .store(&ctx, &store)
            .await?;
        let sub_tree = TestManifest::new()
            .insert("x", leaf())
            .insert("y", leaf())
            .store(&ctx, &store)
            .await?;
        let other_tree = TestManifest::new()
            .insert("z", leaf())
            .store(&ctx, &store)
            .await?;

        // Parent 1:               Parent 2:
        //   shared (leaf)           shared (leaf)
        //   onlyp1 (leaf)           onlyp2 (leaf)
        //   d/{a,b}                 d/{a,b}
        //   sub/{x,y}               other/z
        let root1 = TestManifest::new()
            .insert("shared", leaf())
            .insert("onlyp1", leaf())
            .insert("d", Entry::Tree(d_tree))
            .insert("sub", Entry::Tree(sub_tree))
            .store(&ctx, &store)
            .await?;
        let root2 = TestManifest::new()
            .insert("shared", leaf())
            .insert("onlyp2", leaf())
            .insert("d", Entry::Tree(d_tree))
            .insert("other", Entry::Tree(other_tree))
            .store(&ctx, &store)
            .await?;

        // Two parents: existing only if a file in both.
        let classification = get_implicit_deletes_and_existing_files(
            &ctx,
            store.clone(),
            vec![
                NonRootMPath::new("d/a")?,
                NonRootMPath::new("shared")?,
                NonRootMPath::new("onlyp1")?,
                NonRootMPath::new("onlyp2")?,
                NonRootMPath::new("sub")?,
                NonRootMPath::new("nonexistent")?,
            ],
            vec![root1, root2],
        )
        .await?;
        ensure_unordered_eq(
            classification
                .existing_files_in_all_parents
                .into_iter()
                .collect::<Vec<_>>(),
            vec![NonRootMPath::new("d/a")?, NonRootMPath::new("shared")?],
        );
        ensure_unordered_eq(
            classification.implicit_deletes,
            vec![NonRootMPath::new("sub/x")?, NonRootMPath::new("sub/y")?],
        );

        // Single parent: existing if a file in that parent.
        let classification = get_implicit_deletes_and_existing_files(
            &ctx,
            store.clone(),
            vec![
                NonRootMPath::new("d/a")?,
                NonRootMPath::new("onlyp1")?,
                NonRootMPath::new("onlyp2")?,
                NonRootMPath::new("sub")?,
            ],
            vec![root1],
        )
        .await?;
        ensure_unordered_eq(
            classification
                .existing_files_in_all_parents
                .into_iter()
                .collect::<Vec<_>>(),
            vec![NonRootMPath::new("d/a")?, NonRootMPath::new("onlyp1")?],
        );
        ensure_unordered_eq(
            classification.implicit_deletes,
            vec![NonRootMPath::new("sub/x")?, NonRootMPath::new("sub/y")?],
        );

        // No parents: nothing is an existing file.
        let classification = get_implicit_deletes_and_existing_files(
            &ctx,
            store.clone(),
            vec![NonRootMPath::new("d/a")?],
            Vec::<TestManifestId>::new(),
        )
        .await?;
        assert!(
            classification.existing_files_in_all_parents.is_empty(),
            "a root changeset has no parents, so no path can be an existing file"
        );
        assert!(classification.implicit_deletes.is_empty());

        Ok(())
    }
}

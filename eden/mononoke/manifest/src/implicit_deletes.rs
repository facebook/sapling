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
use futures::future;
use futures::stream;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
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
/// Dir `/p1/p2` was implicitly delted (meaning files `/p1/p2/p3` and
/// `/p1/p2/p4` were implicitly delted)
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
        Manifest<TreeId = ManifestId, LeafId = L> + Send + Sync,
    <<ManifestId as StoreLoadable<Store>>::Value as Manifest>::LeafId: Send + Copy + Eq,
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
        Manifest<TreeId = ManifestId, LeafId = L> + Send + Sync,
    <<ManifestId as StoreLoadable<Store>>::Value as Manifest>::LeafId: Send + Copy + Eq,
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

#[cfg(test)]
mod test {
    use std::fmt::Debug;

    use fbinit::FacebookInit;
    use maplit::hashmap;
    use mononoke_types::FileType;

    use super::*;
    use crate::tests::ctx;
    use crate::tests::dir;
    use crate::tests::element;
    use crate::tests::file;
    use crate::tests::path;
    use crate::tests::ManifestStore;
    use crate::tests::TestManifestIdStr;
    use crate::tests::TestManifestStr;

    fn ensure_unordered_eq<T: Debug + Hash + PartialEq + Eq, I: IntoIterator<Item = T>>(
        v1: I,
        v2: I,
    ) {
        let hs1: HashSet<_> = v1.into_iter().collect();
        let hs2: HashSet<_> = v2.into_iter().collect();
        assert_eq!(hs1, hs2);
    }

    #[fbinit::test]
    async fn test_get_implicit_deletes_single_parent(fb: FacebookInit) -> Result<()> {
        // Parent manifest looks like:
        // p1/
        //   p2/
        //     p3
        //     p4
        //   p5/
        //     p6
        let root_manifest = TestManifestIdStr("1");
        let store = ManifestStore(hashmap! {
            root_manifest => TestManifestStr(hashmap! {
                element("p1") => dir("2"),
            }),
            TestManifestIdStr("2") => TestManifestStr(hashmap! {
                element("p2") => dir("3"),
                element("p5") => dir("5"),
            }),
            TestManifestIdStr("3") => TestManifestStr(hashmap! {
                element("p3") => file(FileType::Regular, "3"),
                element("p4") => file(FileType::Regular, "4"),
            }),
            TestManifestIdStr("5") => TestManifestStr(hashmap! {
                element("p6") => file(FileType::Regular, "6"),
            }),
        });
        // Child adds a file at /p1/p2
        let implicitly_deleted_files: Vec<_> = get_implicit_deletes_single_parent(
            ctx(fb),
            store.clone(),
            vec![path("p1/p2")],
            root_manifest,
        )
        .try_collect()
        .await?;
        // We expect the entire /p1/p2 dir to be implicitly deleted
        // (and we enumerate all files: /p1/p2/p3 and /p1/p2/p4)
        ensure_unordered_eq(
            implicitly_deleted_files,
            vec![path("p1/p2/p3"), path("p1/p2/p4")],
        );

        // Adding an unrelated file should not cause any implicit deletes
        let implicitly_deleted_files: Vec<_> = get_implicit_deletes_single_parent(
            ctx(fb),
            store.clone(),
            vec![path("p1/p200")],
            root_manifest,
        )
        .try_collect()
        .await?;
        assert_eq!(implicitly_deleted_files, vec![]);

        Ok(())
    }

    #[fbinit::test]
    async fn test_implicit_deletes(fb: FacebookInit) -> Result<()> {
        // Parent 1 manifest looks like:
        // p1/
        //   p2/
        //     p3
        //     p4
        //   p5/
        //     p6
        //   p7
        // p8
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
        let root_manifest_1 = TestManifestIdStr("1_1");
        let root_manifest_2 = TestManifestIdStr("1_2");
        let store = ManifestStore(hashmap! {
            root_manifest_1 => TestManifestStr(hashmap! {
                element("p1") => dir("2_1"),
                element("p8") => file(FileType::Regular, "8_1"),
            }),
            TestManifestIdStr("2_1") => TestManifestStr(hashmap! {
                element("p2") => dir("3"),
                element("p5") => dir("5"),
                element("p7") => file(FileType::Regular, "7_1"),
            }),
            root_manifest_2 => TestManifestStr(hashmap! {
                element("p1") => dir("2_2"),
                element("p11") => file(FileType::Regular, "11_1"),
            }),
            TestManifestIdStr("2_2") => TestManifestStr(hashmap! {
                element("p2") => dir("3"),
                element("p5") => dir("5"),
                element("p9") => dir("9_2")
            }),
            TestManifestIdStr("9_2") => TestManifestStr(hashmap! {
                element("p10") => file(FileType::Executable, "10_2"),
            }),
            TestManifestIdStr("3") => TestManifestStr(hashmap! {
                element("p3") => file(FileType::Regular, "3"),
                element("p4") => file(FileType::Regular, "4"),
            }),
            TestManifestIdStr("5") => TestManifestStr(hashmap! {
                element("p6") => file(FileType::Regular, "6"),
            }),
        });
        let c = ctx(fb);
        // Child adds files at /p1/p2, /p1/p7/p12 and /p1/p9
        let implicitly_deleted_files: Vec<_> = get_implicit_deletes(
            &c,
            store.clone(),
            vec![path("p1/p2"), path("p1/p7/p12"), path("p1/p9")],
            vec![root_manifest_1, root_manifest_2],
        )
        .try_collect()
        .await?;

        ensure_unordered_eq(
            implicitly_deleted_files,
            vec![path("p1/p2/p3"), path("p1/p2/p4"), path("p1/p9/p10")],
        );

        // Result should not depend on the order of parents
        let implicitly_deleted_files: Vec<_> = get_implicit_deletes(
            &c,
            store,
            vec![path("p1/p2"), path("p1/p7/p12"), path("p1/p9")],
            vec![root_manifest_2, root_manifest_1],
        )
        .try_collect()
        .await?;

        ensure_unordered_eq(
            implicitly_deleted_files,
            vec![path("p1/p2/p3"), path("p1/p2/p4"), path("p1/p9/p10")],
        );
        Ok(())
    }
}

/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use cloned::cloned;
use context::CoreContext;
use futures::{stream, Future, Stream};
use futures_ext::StreamExt;
use mononoke_types::{FileType, MPath};
use std::collections::HashSet;
use std::hash::Hash;

use crate::types::StoreLoadable;
use crate::{Entry, Manifest, ManifestOps};

/// Get implicit directory deletes
/// These may happen when a path, occupied by a dir in the parent manifest
/// becomes occupied by a file in the child. In this case all files in the
/// replaced directory are implicitly deleted.
/// For example, consider the following manifest:
/// ```
/// p1/
///   p2/   <- a dir
///     p3  <- a file
///     p4  <- a file
/// ```
/// Now a child adds the following file: `/p1/p2`
/// New manifest looks like:
/// ```
/// p1/
///   p2    <- f file
/// ```
/// Dir `/p1/p2` was implicitly delted (meaning files `/p1/p2/p3` and
/// `/p1/p2/p4` were implicitly delted)
fn get_implicit_dir_deletes<ManifestId, FileId, Store>(
    ctx: CoreContext,
    store: Store,
    path_added_in_a_child: MPath,
    parent: ManifestId,
) -> impl Stream<Item = MPath, Error = Error>
where
    FileId: Hash + Eq + Send + Sync + 'static,
    ManifestId: Hash + Eq + StoreLoadable<Store> + Send + Sync + ManifestOps<Store> + 'static,
    Store: Send + Sync + Clone + 'static,
    <ManifestId as StoreLoadable<Store>>::Value:
        Manifest<TreeId = ManifestId, LeafId = (FileType, FileId)> + Send + Sync,
    <<ManifestId as StoreLoadable<Store>>::Value as Manifest>::LeafId: Send + Copy + Eq,
{
    parent
        .find_entry(
            ctx.clone(),
            store.clone(),
            Some(path_added_in_a_child.clone()),
        )
        .map({
            cloned!(ctx, store, path_added_in_a_child);
            move |maybe_entry| {
                match maybe_entry {
                    // No such path in parent or such path used to be a leaf.
                    // No implicit delete in both cases
                    None | Some(Entry::Leaf(_)) => stream::empty().left_stream(),
                    Some(Entry::Tree(tree)) => tree
                        .list_leaf_entries(ctx, store)
                        .map({
                            cloned!(path_added_in_a_child);
                            move |(relative_path, _)| path_added_in_a_child.join(&relative_path)
                        })
                        .right_stream(),
                }
            }
        })
        .flatten_stream()
}

/// Get implicit deletes from a single parent manifest,
/// caused by introducing new paths into a child
fn get_implicit_deletes_single_parent<ManifestId, FileId, Store, I>(
    ctx: CoreContext,
    store: Store,
    paths_added_in_a_child: I,
    parent: ManifestId,
) -> impl Stream<Item = MPath, Error = Error>
where
    FileId: Hash + Eq + Send + Sync + 'static,
    ManifestId: Hash + Eq + StoreLoadable<Store> + Send + Sync + ManifestOps<Store> + 'static,
    Store: Send + Sync + Clone + 'static,
    <ManifestId as StoreLoadable<Store>>::Value:
        Manifest<TreeId = ManifestId, LeafId = (FileType, FileId)> + Send + Sync,
    <<ManifestId as StoreLoadable<Store>>::Value as Manifest>::LeafId: Send + Copy + Eq,
    I: IntoIterator<Item = MPath>,
{
    let individual_path_streams = paths_added_in_a_child.into_iter().map({
        cloned!(ctx, store, parent);
        move |path_added_in_a_child| {
            get_implicit_dir_deletes(
                ctx.clone(),
                store.clone(),
                path_added_in_a_child,
                parent.clone(),
            )
        }
    });
    stream::iter_ok::<_, Error>(individual_path_streams).flatten()
}

/// Get implicit deletes in parent manifests,
/// caused by introducing new paths into a child
pub fn get_implicit_deletes<ManifestId, FileId, Store, I, M>(
    ctx: CoreContext,
    store: Store,
    paths_added_in_a_child: I,
    parents: M,
) -> impl Stream<Item = MPath, Error = Error>
where
    FileId: Hash + Eq + Send + Sync + 'static,
    ManifestId: Hash + Eq + StoreLoadable<Store> + Send + Sync + ManifestOps<Store> + 'static,
    Store: Send + Sync + Clone + 'static,
    <ManifestId as StoreLoadable<Store>>::Value:
        Manifest<TreeId = ManifestId, LeafId = (FileType, FileId)> + Send + Sync,
    <<ManifestId as StoreLoadable<Store>>::Value as Manifest>::LeafId: Send + Copy + Eq,
    I: IntoIterator<Item = MPath> + Clone,
    M: IntoIterator<Item = ManifestId>,
{
    let individual_parent_streams = parents.into_iter().map({
        cloned!(ctx, store, paths_added_in_a_child);
        move |parent| {
            get_implicit_deletes_single_parent(
                ctx.clone(),
                store.clone(),
                paths_added_in_a_child.clone(),
                parent,
            )
        }
    });
    // Note that identical implicit deletes may exist against
    // multiple parents, so to ensure uniqueness, we must allocate
    // some additional memory
    stream::iter_ok::<_, Error>(individual_parent_streams)
        .flatten()
        .filter({
            let mut seen = HashSet::new();
            move |item| seen.insert(item.clone())
        })
}

#[cfg(test)]
mod test {
    use super::*;
    use anyhow::format_err;
    use fbinit::{self, FacebookInit};
    use futures::IntoFuture;
    use futures_ext::{BoxFuture, FutureExt as Futures01FutureExt};
    use futures_util::compat::Future01CompatExt;
    use maplit::hashmap;
    use mononoke_types::MPathElement;
    use std::collections::HashMap;
    use std::fmt::Debug;
    use tokio_preview as tokio;

    type BonsaiEntry<ManifestId, FileId> = Entry<ManifestId, (FileType, FileId)>;

    #[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
    struct TestManifestId(&'static str);

    #[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
    struct TestFileId(&'static str);

    #[derive(Default, Clone, Debug)]
    struct TestManifest(HashMap<MPathElement, BonsaiEntry<TestManifestId, TestFileId>>);

    #[derive(Default, Debug, Clone)]
    struct ManifestStore(HashMap<TestManifestId, TestManifest>);

    impl StoreLoadable<ManifestStore> for TestManifestId {
        type Value = TestManifest;

        fn load(&self, _ctx: CoreContext, store: &ManifestStore) -> BoxFuture<Self::Value, Error> {
            store
                .0
                .get(&self)
                .cloned()
                .ok_or(format_err!("manifest not in store {}", self.0))
                .into_future()
                .boxify()
        }
    }

    impl Manifest for TestManifest {
        type TreeId = TestManifestId;
        type LeafId = (FileType, TestFileId);

        fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
            self.0.get(name).cloned()
        }

        fn list(
            &self,
        ) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
            Box::new(self.0.clone().into_iter())
        }
    }

    fn ctx(fb: FacebookInit) -> CoreContext {
        CoreContext::test_mock(fb)
    }

    fn element(s: &str) -> MPathElement {
        MPathElement::new(s.as_bytes().iter().cloned().collect()).unwrap()
    }

    fn path(s: &str) -> MPath {
        MPath::new(s).unwrap()
    }

    fn file(ty: FileType, name: &'static str) -> BonsaiEntry<TestManifestId, TestFileId> {
        BonsaiEntry::Leaf((ty, TestFileId(name)))
    }

    fn dir(name: &'static str) -> BonsaiEntry<TestManifestId, TestFileId> {
        BonsaiEntry::Tree(TestManifestId(name))
    }

    fn ensure_unordered_eq<T: Debug + Hash + PartialEq + Eq, I: IntoIterator<Item = T>>(
        v1: I,
        v2: I,
    ) {
        let hs1: HashSet<_> = v1.into_iter().collect();
        let hs2: HashSet<_> = v2.into_iter().collect();
        assert_eq!(hs1, hs2);
    }

    #[fbinit::test]
    async fn test_implicit_dir_deletes(fb: FacebookInit) -> Result<(), Error> {
        // Parent manifest looks like:
        // p1/
        //   p2/
        //     p3
        //     p4
        //   p5/
        //     p6
        let root_manifest = TestManifestId("1");
        let store = ManifestStore(hashmap! {
            root_manifest => TestManifest(hashmap! {
                element("p1") => dir("2"),
            }),
            TestManifestId("2") => TestManifest(hashmap! {
                element("p2") => dir("3"),
                element("p5") => dir("5"),
            }),
            TestManifestId("3") => TestManifest(hashmap! {
                element("p3") => file(FileType::Regular, "3"),
                element("p4") => file(FileType::Regular, "4"),
            }),
            TestManifestId("5") => TestManifest(hashmap! {
                element("p6") => file(FileType::Regular, "6"),
            }),
        });
        // Child adds a file at /p1/p2
        let implicitly_deleted_files =
            get_implicit_dir_deletes(ctx(fb), store.clone(), path("p1/p2"), root_manifest)
                .collect()
                .compat()
                .await?;
        // We expect the entire /p1/p2 dir to be implicitly deleted
        // (and we enumerate all files: /p1/p2/p3 and /p1/p2/p4)
        ensure_unordered_eq(
            implicitly_deleted_files,
            vec![path("p1/p2/p3"), path("p1/p2/p4")],
        );

        // Adding an unrelated file should not cause any implicit deletes
        let implicitly_deleted_files =
            get_implicit_dir_deletes(ctx(fb), store.clone(), path("p1/p200"), root_manifest)
                .collect()
                .compat()
                .await?;
        assert_eq!(implicitly_deleted_files, vec![]);

        Ok(())
    }

    #[fbinit::test]
    async fn test_implicit_deletes(fb: FacebookInit) -> Result<(), Error> {
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
        let root_manifest_1 = TestManifestId("1_1");
        let root_manifest_2 = TestManifestId("1_2");
        let store = ManifestStore(hashmap! {
            root_manifest_1 => TestManifest(hashmap! {
                element("p1") => dir("2_1"),
                element("p8") => file(FileType::Regular, "8_1"),
            }),
            TestManifestId("2_1") => TestManifest(hashmap! {
                element("p2") => dir("3"),
                element("p5") => dir("5"),
                element("p7") => file(FileType::Regular, "7_1"),
            }),
            root_manifest_2 => TestManifest(hashmap! {
                element("p1") => dir("2_2"),
                element("p11") => file(FileType::Regular, "11_1"),
            }),
            TestManifestId("2_2") => TestManifest(hashmap! {
                element("p2") => dir("3"),
                element("p5") => dir("5"),
                element("p9") => dir("9_2")
            }),
            TestManifestId("9_2") => TestManifest(hashmap! {
                element("p10") => file(FileType::Executable, "10_2"),
            }),
            TestManifestId("3") => TestManifest(hashmap! {
                element("p3") => file(FileType::Regular, "3"),
                element("p4") => file(FileType::Regular, "4"),
            }),
            TestManifestId("5") => TestManifest(hashmap! {
                element("p6") => file(FileType::Regular, "6"),
            }),
        });
        // Child adds files at /p1/p2, /p1/p7/p12 and /p1/p9
        let implicitly_deleted_files = get_implicit_deletes(
            ctx(fb),
            store.clone(),
            vec![path("p1/p2"), path("p1/p7/p12"), path("p1/p9")],
            vec![root_manifest_1, root_manifest_2],
        )
        .collect()
        .compat()
        .await?;

        ensure_unordered_eq(
            implicitly_deleted_files,
            vec![path("p1/p2/p3"), path("p1/p2/p4"), path("p1/p9/p10")],
        );

        // Result should not depend on the order of parents
        let implicitly_deleted_files = get_implicit_deletes(
            ctx(fb),
            store,
            vec![path("p1/p2"), path("p1/p7/p12"), path("p1/p9")],
            vec![root_manifest_2, root_manifest_1],
        )
        .collect()
        .compat()
        .await?;

        ensure_unordered_eq(
            implicitly_deleted_files,
            vec![path("p1/p2/p3"), path("p1/p2/p4"), path("p1/p9/p10")],
        );
        Ok(())
    }

}

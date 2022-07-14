/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::Entry;
use crate::Manifest;
use anyhow::Error;
use blobstore::StoreLoadable;
use cloned::cloned;
use context::CoreContext;
use futures::future;
use futures::future::try_join_all;
use futures::try_join;
use futures::FutureExt;
use futures::Stream;
use futures::TryFutureExt;
use futures::TryStreamExt;
use maplit::hashmap;
use maplit::hashset;
use mononoke_types::FileType;
use mononoke_types::MPath;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Debug;
use std::hash::Hash;
use tokio::task;

pub(crate) type BonsaiEntry<ManifestId, FileId> = Entry<ManifestId, (FileType, FileId)>;

#[derive(Clone, Eq, Debug, Hash, PartialEq, PartialOrd, Ord)]
pub enum BonsaiDiffFileChange<FileId> {
    /// This file was changed (was added or modified) in this changeset.
    Changed(MPath, FileType, FileId),

    /// The file was marked changed, but one of the parent file ID was reused. This can happen in
    /// these situations:
    ///
    /// 1. The file type was changed without a corresponding change in file contents.
    /// 2. There's a merge and one of the parent nodes was picked as the resolution.
    ///
    /// This is separate from `Changed` because in these instances, if copy information is part of
    /// the node it wouldn't be recorded.
    ChangedReusedId(MPath, FileType, FileId),

    /// This file was deleted in this changeset.
    Deleted(MPath),
}

impl<FileId> BonsaiDiffFileChange<FileId> {
    pub fn path(&self) -> &MPath {
        match self {
            Self::Changed(path, ..) | Self::ChangedReusedId(path, ..) | Self::Deleted(path) => path,
        }
    }
    pub fn file_id(&self) -> Option<&FileId> {
        match self {
            Self::Changed(_, _, fid) | Self::ChangedReusedId(_, _, fid) => Some(fid),
            Self::Deleted(_) => None,
        }
    }
    pub fn file_type(&self) -> Option<&FileType> {
        match self {
            Self::Changed(_, ft, _) | Self::ChangedReusedId(_, ft, _) => Some(ft),
            Self::Deleted(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompositeEntry<ManifestId, FileId>
where
    FileId: Hash + Eq,
    ManifestId: Hash + Eq,
{
    manifests: HashSet<ManifestId>,
    files: HashSet<(FileType, FileId)>,
}

impl<ManifestId, FileId> CompositeEntry<ManifestId, FileId>
where
    FileId: Hash + Eq,
    ManifestId: Hash + Eq,
{
    fn empty() -> Self {
        Self {
            manifests: hashset! {},
            files: hashset! {},
        }
    }

    fn insert(&mut self, entry: BonsaiEntry<ManifestId, FileId>) {
        match entry {
            Entry::Leaf(new_id) => {
                self.files.insert(new_id);
            }
            Entry::Tree(new_id) => {
                self.manifests.insert(new_id);
            }
        }
    }

    fn into_parts(
        self,
    ) -> (
        Option<HashSet<ManifestId>>,
        Option<HashSet<(FileType, FileId)>>,
    ) {
        let manifests = if self.manifests.is_empty() {
            None
        } else {
            Some(self.manifests)
        };

        let files = if self.files.is_empty() {
            None
        } else {
            Some(self.files)
        };

        (manifests, files)
    }
}

/// WorkEntry describes the work to be performed by the bounded_traversal to produce a Bonsai diff.
/// It maps a path to consider to the contents of the Manifest for which to produce Bonsai changes
/// at this path and the contents of the parents at this path.
type WorkEntry<ManifestId, FileId> = HashMap<
    MPath,
    (
        Option<BonsaiEntry<ManifestId, FileId>>,
        CompositeEntry<ManifestId, FileId>,
    ),
>;

/// Identify further work to be performed for this Bonsai diff.
async fn recurse_trees<ManifestId, FileId, Store>(
    ctx: &CoreContext,
    store: &Store,
    path: Option<&MPath>,
    node: Option<ManifestId>,
    parents: HashSet<ManifestId>,
) -> Result<WorkEntry<ManifestId, FileId>, Error>
where
    FileId: Hash + Eq,
    ManifestId: Hash + Eq + StoreLoadable<Store>,
    <ManifestId as StoreLoadable<Store>>::Value:
        Manifest<TreeId = ManifestId, LeafId = (FileType, FileId)>,
{
    // If there is a single parent, and it's unchanged, then we don't need to recurse.
    if parents.len() == 1 && node.as_ref() == parents.iter().next() {
        return Ok(HashMap::new());
    }

    let node = async {
        let r = match node {
            Some(node) => Some(node.load(ctx, store).await?),
            None => None,
        };
        Ok(r)
    };

    let parents = try_join_all(
        parents
            .into_iter()
            .map(|id| async move { id.load(ctx, store).await }),
    );

    let (node, parents) = try_join!(node, parents)?;

    let mut ret = HashMap::new();

    if let Some(node) = node {
        for (path, entry) in node.list() {
            ret.entry(path).or_insert((None, CompositeEntry::empty())).0 = Some(entry);
        }
    }

    for parent in parents {
        for (path, entry) in parent.list() {
            ret.entry(path)
                .or_insert((None, CompositeEntry::empty()))
                .1
                .insert(entry);
        }
    }

    let ret = ret
        .into_iter()
        .map(|(p, e)| {
            let p = MPath::join_opt_element(path, &p);
            (p, e)
        })
        .collect();

    Ok(ret)
}

fn resolve_file_over_files<FileId>(
    path: MPath,
    node: (FileType, FileId),
    parents: HashSet<(FileType, FileId)>,
) -> Option<BonsaiDiffFileChange<FileId>>
where
    FileId: Hash + Eq,
{
    if parents.len() == 1 && parents.contains(&node) {
        return None;
    }

    Some(resolve_file_over_mixed(path, node, parents))
}

fn resolve_file_over_mixed<FileId>(
    path: MPath,
    node: (FileType, FileId),
    parents: HashSet<(FileType, FileId)>,
) -> BonsaiDiffFileChange<FileId>
where
    FileId: Hash + Eq,
{
    if parents.iter().any(|e| e.1 == node.1) {
        return BonsaiDiffFileChange::ChangedReusedId(path, node.0, node.1);
    }

    BonsaiDiffFileChange::Changed(path, node.0, node.1)
}

async fn bonsai_diff_unfold<ManifestId, FileId, Store>(
    ctx: &CoreContext,
    store: &Store,
    path: MPath,
    node: Option<BonsaiEntry<ManifestId, FileId>>,
    parents: CompositeEntry<ManifestId, FileId>,
) -> Result<
    (
        Option<BonsaiDiffFileChange<FileId>>,
        WorkEntry<ManifestId, FileId>,
    ),
    Error,
>
where
    FileId: Hash + Eq,
    ManifestId: Hash + Eq + StoreLoadable<Store>,
    <ManifestId as StoreLoadable<Store>>::Value:
        Manifest<TreeId = ManifestId, LeafId = (FileType, FileId)>,
{
    let res = match node {
        Some(Entry::Tree(node)) => {
            let (manifests, files) = parents.into_parts();

            // We have a tree here, so we'll need to compare it to the matching manifests in the parent
            // manifests.
            let recurse = recurse_trees(
                ctx,
                store,
                Some(&path),
                Some(node),
                manifests.unwrap_or_default(),
            )
            .await?;

            // If the parent manifests contained a file at the path where we have a tree, then we
            // must indicate that this path is being deleted, since otherwise we'd have an invalid
            // Bonsai changeset that contains files that are children of a file!
            let change = if files.is_some() {
                Some(BonsaiDiffFileChange::Deleted(path))
            } else {
                None
            };

            (change, recurse)
        }
        Some(Entry::Leaf(node)) => {
            // We have a file here. We won't need to recurse into the parents: the presence of this
            // file implicitly deletes all descendent files (if any exist).
            let recurse = hashmap! {};

            let change = match parents.into_parts() {
                // At least one of our parents has a manifest here. We must emit a file to delete
                // it.
                (Some(_trees), Some(files)) => Some(resolve_file_over_mixed(path, node, files)),
                // Our parents have only files at this path. We might not need to emit anything if
                // they have the same files.
                (None, Some(files)) => resolve_file_over_files(path, node, files),
                // We don't have files in the parents. Regardless of whether we have manifests, we'll
                // need to emit this file, so let's do so.
                (_, None) => Some(BonsaiDiffFileChange::Changed(path, node.0, node.1)),
            };

            (change, recurse)
        }
        None => {
            // We don't have anything at this path, but our parents do: we need to recursively
            // delete everything in this tree.
            let (manifests, files) = parents.into_parts();

            let recurse =
                recurse_trees(ctx, store, Some(&path), None, manifests.unwrap_or_default()).await?;

            let change = if files.is_some() {
                Some(BonsaiDiffFileChange::Deleted(path))
            } else {
                None
            };

            (change, recurse)
        }
    };

    Ok(res)
}

pub fn bonsai_diff<ManifestId, FileId, Store>(
    ctx: CoreContext,
    store: Store,
    node: ManifestId,
    parents: HashSet<ManifestId>,
) -> impl Stream<Item = Result<BonsaiDiffFileChange<FileId>, Error>>
where
    FileId: Hash + Eq + Send + Sync + 'static,
    ManifestId: Hash + Eq + StoreLoadable<Store> + Send + Sync + 'static,
    Store: Send + Sync + Clone + 'static,
    <ManifestId as StoreLoadable<Store>>::Value:
        Manifest<TreeId = ManifestId, LeafId = (FileType, FileId)> + Send + Sync,
{
    // NOTE: `async move` blocks are used below to own CoreContext and Store for the (static)
    // lifetime of the stream we're returning here (recurse_trees and bonsai_diff_unfold don't
    // require those).
    {
        cloned!(ctx, store);
        async move { recurse_trees(&ctx, &store, None, Some(node), parents).await }
    }
    .map_ok(|seed| {
        bounded_traversal::bounded_traversal_stream(256, seed, move |(path, (node, parents))| {
            cloned!(ctx, store);
            async move {
                task::spawn(
                    async move { bonsai_diff_unfold(&ctx, &store, path, node, parents).await },
                )
                .await?
            }
            .boxed()
        })
    })
    .try_flatten_stream()
    .try_filter_map(future::ok)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::tests::ctx;
    use crate::tests::dir;
    use crate::tests::element;
    use crate::tests::file;
    use crate::tests::path;
    use crate::tests::ManifestStore;
    use crate::tests::TestFileId;
    use crate::tests::TestManifestIdStr;
    use crate::tests::TestManifestStr;
    use borrowed::borrowed;
    use fbinit::FacebookInit;

    impl<ManifestId, FileId> CompositeEntry<ManifestId, FileId>
    where
        FileId: Hash + Eq,
        ManifestId: Hash + Eq,
    {
        fn files(files: HashSet<(FileType, FileId)>) -> Self {
            Self {
                manifests: hashset! {},
                files,
            }
        }

        fn manifests(manifests: HashSet<ManifestId>) -> Self {
            Self {
                manifests,
                files: hashset! {},
            }
        }
    }

    fn changed(
        path: MPath,
        ty: FileType,
        name: &'static str,
    ) -> Option<BonsaiDiffFileChange<TestFileId>> {
        Some(BonsaiDiffFileChange::Changed(path, ty, TestFileId(name)))
    }

    fn reused(
        path: MPath,
        ty: FileType,
        name: &'static str,
    ) -> Option<BonsaiDiffFileChange<TestFileId>> {
        Some(BonsaiDiffFileChange::ChangedReusedId(
            path,
            ty,
            TestFileId(name),
        ))
    }

    fn deleted(path: MPath) -> Option<BonsaiDiffFileChange<TestFileId>> {
        Some(BonsaiDiffFileChange::Deleted(path))
    }

    #[fbinit::test]
    async fn test_unfold_file_from_files(fb: FacebookInit) -> Result<(), Error> {
        let store = ManifestStore::default();
        let root = path("a");

        let node = Some(file(FileType::Regular, "1"));
        let mut parents = CompositeEntry::empty();
        let ctx = ctx(fb);
        borrowed!(ctx);

        // Start with no parent. The file should be added.
        let (change, work) =
            bonsai_diff_unfold(ctx, &store, root.clone(), node, parents.clone()).await?;
        assert_eq!(change, changed(root.clone(), FileType::Regular, "1"));
        assert_eq!(work, hashmap! {});

        // Add the same file in a parent
        parents.insert(file(FileType::Regular, "1"));
        let (change, work) =
            bonsai_diff_unfold(ctx, &store, root.clone(), node, parents.clone()).await?;
        assert_eq!(change, None);
        assert_eq!(work, hashmap! {});

        // Add the file again in a different parent
        parents.insert(file(FileType::Regular, "1"));
        let (change, work) =
            bonsai_diff_unfold(ctx, &store, root.clone(), node, parents.clone()).await?;
        assert_eq!(change, None);
        assert_eq!(work, hashmap! {});

        // Add a different file
        parents.insert(file(FileType::Regular, "2"));
        let (change, work) =
            bonsai_diff_unfold(ctx, &store, root.clone(), node, parents.clone()).await?;
        assert_eq!(change, reused(root.clone(), FileType::Regular, "1"));
        assert_eq!(work, hashmap! {});

        Ok(())
    }

    #[fbinit::test]
    async fn test_unfold_file_mode_change(fb: FacebookInit) -> Result<(), Error> {
        let store = ManifestStore::default();
        let root = path("a");

        let node = Some(file(FileType::Regular, "1"));
        let mut parents = CompositeEntry::empty();
        let ctx = ctx(fb);
        borrowed!(ctx);

        // Add a parent with a different mode. We can reuse it.
        parents.insert(file(FileType::Executable, "1"));
        let (change, work) =
            bonsai_diff_unfold(ctx, &store, root.clone(), node, parents.clone()).await?;
        assert_eq!(change, reused(root.clone(), FileType::Regular, "1"));
        assert_eq!(work, hashmap! {});

        Ok(())
    }

    #[fbinit::test]
    async fn test_unfold_file_from_dirs(fb: FacebookInit) -> Result<(), Error> {
        let store = ManifestStore::default();
        let root = path("a");

        let node = Some(file(FileType::Regular, "1"));
        let mut parents = CompositeEntry::empty();
        let ctx = ctx(fb);
        borrowed!(ctx);

        // Add a conflicting directory. We need to delete it.
        parents.insert(dir("1"));
        let (change, work) =
            bonsai_diff_unfold(ctx, &store, root.clone(), node, parents.clone()).await?;
        assert_eq!(change, changed(root.clone(), FileType::Regular, "1"));
        assert_eq!(work, hashmap! {});

        // Add another parent with the same file. We can reuse it but we still need to emit it.
        parents.insert(file(FileType::Regular, "1"));
        let (change, work) =
            bonsai_diff_unfold(ctx, &store, root.clone(), node, parents.clone()).await?;
        assert_eq!(change, reused(root.clone(), FileType::Regular, "1"));
        assert_eq!(work, hashmap! {});

        // Add a different file. Same as above.
        parents.insert(file(FileType::Regular, "2"));
        let (change, work) =
            bonsai_diff_unfold(ctx, &store, root.clone(), node, parents.clone()).await?;
        assert_eq!(change, reused(root.clone(), FileType::Regular, "1"));
        assert_eq!(work, hashmap! {});

        Ok(())
    }

    #[fbinit::test]
    async fn test_unfold_dir_from_dirs(fb: FacebookInit) -> Result<(), Error> {
        let root = path("a");

        let store = ManifestStore(hashmap! {
            TestManifestIdStr("1") => TestManifestStr(hashmap! {
                element("p1") => file(FileType::Regular, "1"),
            }),
            TestManifestIdStr("2") => TestManifestStr(hashmap! {
                element("p1") => file(FileType::Executable, "2"),
                element("p2") => dir("2"),
            })
        });

        let node = Some(dir("1"));
        let mut parents = CompositeEntry::empty();
        let ctx = ctx(fb);
        borrowed!(ctx);

        // No parents. We need to recurse in this directory.
        let (change, work) =
            bonsai_diff_unfold(ctx, &store, root.clone(), node, parents.clone()).await?;
        assert_eq!(change, None);
        assert_eq!(
            work,
            hashmap! {
                path("a/p1") => (Some(file(FileType::Regular, "1")), CompositeEntry::empty()),
            }
        );

        // Identical parent. We are done.
        parents.insert(dir("1"));
        let (change, work) =
            bonsai_diff_unfold(ctx, &store, root.clone(), node, parents.clone()).await?;
        assert_eq!(change, None);
        assert_eq!(work, hashmap! {});

        // One parent differs. Recurse on the 2 paths reachable from those manifests, and with the
        // contents listed in both parent manifests.
        parents.insert(dir("2"));
        let (change, work) =
            bonsai_diff_unfold(ctx, &store, root.clone(), node, parents.clone()).await?;
        assert_eq!(change, None);
        assert_eq!(
            work,
            hashmap! {
                path("a/p1") => (
                    Some(file(FileType::Regular, "1")),
                    CompositeEntry::files(hashset! {
                        (FileType::Regular, TestFileId("1")),
                        (FileType::Executable, TestFileId("2"))
                    })
                ),
                path("a/p2") => (
                    None,
                    CompositeEntry::manifests(hashset! { TestManifestIdStr("2") })
                ),
            }
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_unfold_dir_from_files(fb: FacebookInit) -> Result<(), Error> {
        let root = path("a");

        let store =
            ManifestStore(hashmap! { TestManifestIdStr("1") => TestManifestStr(hashmap! {}) });

        let node = Some(dir("1"));
        let mut parents = CompositeEntry::empty();
        let ctx = ctx(fb);
        borrowed!(ctx);

        // Parent has a file. Delete it.
        parents.insert(file(FileType::Regular, "1"));
        let (change, work) =
            bonsai_diff_unfold(ctx, &store, root.clone(), node, parents.clone()).await?;
        assert_eq!(change, deleted(root));
        assert_eq!(work, hashmap! {});

        Ok(())
    }

    #[fbinit::test]
    async fn test_unfold_missing_from_files(fb: FacebookInit) -> Result<(), Error> {
        let store = ManifestStore::default();
        let root = path("a");

        let mut parents = CompositeEntry::empty();
        let ctx = ctx(fb);
        borrowed!(ctx);

        // Parent has a file, delete it.
        parents.insert(file(FileType::Regular, "1"));
        let (change, work) =
            bonsai_diff_unfold(ctx, &store, root.clone(), None, parents.clone()).await?;
        assert_eq!(change, deleted(root));
        assert_eq!(work, hashmap! {});

        Ok(())
    }

    #[fbinit::test]
    async fn test_unfold_missing_from_dirs(fb: FacebookInit) -> Result<(), Error> {
        let root = path("a");

        let store = ManifestStore(hashmap! {
            TestManifestIdStr("1") => TestManifestStr(hashmap! {
                element("p1") => dir("2"),
            }),
            TestManifestIdStr("2") => TestManifestStr(hashmap! {
                element("p2") => dir("3"),
            })
        });

        let mut parents = CompositeEntry::empty();
        let ctx = ctx(fb);
        borrowed!(ctx);

        // Parent has a directory, recurse into it.
        parents.insert(dir("1"));
        let (change, work) =
            bonsai_diff_unfold(ctx, &store, root.clone(), None, parents.clone()).await?;
        assert_eq!(change, None);
        assert_eq!(
            work,
            hashmap! {
                path("a/p1") => (
                    None,
                    CompositeEntry::manifests(hashset! { TestManifestIdStr("2") })
                ),
            }
        );

        // Multiple parents have multiple directories. Recurse into all of them.
        parents.insert(dir("2"));
        let (change, work) =
            bonsai_diff_unfold(ctx, &store, root.clone(), None, parents.clone()).await?;
        assert_eq!(change, None);
        assert_eq!(
            work,
            hashmap! {
                path("a/p1") => (
                    None,
                    CompositeEntry::manifests(hashset! { TestManifestIdStr("2") })
                ),
                path("a/p2") => (
                    None,
                    CompositeEntry::manifests(hashset! { TestManifestIdStr("3") })
                ),
            }
        );

        Ok(())
    }
}

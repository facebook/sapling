/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub(crate) use crate::{
    bonsai::BonsaiEntry, derive_manifest, find_intersection_of_diffs, Diff, Entry, Manifest,
    ManifestOps, PathOrPrefix, PathTree, TreeInfo,
};
use anyhow::{Error, Result};
use blobstore::{Blobstore, Loadable, LoadableError, Storable, StoreLoadable};
use bounded_traversal::bounded_traversal_stream;
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{
    future::{self, BoxFuture, Future, FutureExt},
    stream::TryStreamExt,
};
use maplit::btreemap;
use memblob::LazyMemblob;
use mononoke_types::{BlobstoreBytes, FileType, MPath, MPathElement};
use pretty_assertions::assert_eq;
use serde_derive::{Deserialize, Serialize};
use std::{
    collections::{hash_map::DefaultHasher, BTreeMap, BTreeSet, HashMap},
    hash::{Hash, Hasher},
    iter::FromIterator,
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct TestLeafId(u64);

#[derive(Debug)]
struct TestLeaf(String);

impl Loadable for TestLeafId {
    type Value = TestLeaf;

    fn load<'a, B: Blobstore>(
        &'a self,
        ctx: CoreContext,
        blobstore: &'a B,
    ) -> BoxFuture<'a, Result<Self::Value, LoadableError>> {
        let key = self.0.to_string();
        let get = blobstore.get(ctx, key.clone());

        async move {
            let bytes = get.await?.ok_or(LoadableError::Missing(key))?;
            let bytes = std::str::from_utf8(bytes.as_raw_bytes())
                .map_err(|err| LoadableError::Error(Error::from(err)))?;
            Ok(TestLeaf(bytes.to_owned()))
        }
        .boxed()
    }
}

impl Storable for TestLeaf {
    type Key = TestLeafId;

    fn store<B: Blobstore>(
        self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<'_, Result<Self::Key, Error>> {
        let mut hasher = DefaultHasher::new();
        self.0.hash(&mut hasher);
        let key = TestLeafId(hasher.finish());
        let put = blobstore.put(ctx, key.0.to_string(), BlobstoreBytes::from_bytes(self.0));

        async move {
            put.await?;
            Ok(key)
        }
        .boxed()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct TestManifestIdU64(u64);

#[derive(Debug, Serialize, Deserialize)]
struct TestManifestU64(BTreeMap<MPathElement, Entry<TestManifestIdU64, TestLeafId>>);

impl Loadable for TestManifestIdU64 {
    type Value = TestManifestU64;

    fn load<'a, B: Blobstore>(
        &'a self,
        ctx: CoreContext,
        blobstore: &'a B,
    ) -> BoxFuture<'a, Result<Self::Value, LoadableError>> {
        let key = self.0.to_string();
        let get = blobstore.get(ctx, key.clone());
        async move {
            let data = get.await?;
            let bytes = data.ok_or(LoadableError::Missing(key))?;
            let mf = serde_cbor::from_slice(bytes.as_raw_bytes().as_ref())
                .map_err(|err| LoadableError::Error(Error::from(err)))?;
            Ok(mf)
        }
        .boxed()
    }
}

impl Storable for TestManifestU64 {
    type Key = TestManifestIdU64;

    fn store<B: Blobstore>(
        self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<'_, Result<Self::Key, Error>> {
        let blobstore = blobstore.clone();
        let mut hasher = DefaultHasher::new();
        self.0.hash(&mut hasher);
        let key = TestManifestIdU64(hasher.finish());

        async move {
            let bytes = serde_cbor::to_vec(&self)?;
            blobstore
                .put(ctx, key.0.to_string(), BlobstoreBytes::from_bytes(bytes))
                .await?;
            Ok(key)
        }
        .boxed()
    }
}

impl Manifest for TestManifestU64 {
    type LeafId = TestLeafId;
    type TreeId = TestManifestIdU64;

    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
        let iter = self.0.clone().into_iter();
        Box::new(iter)
    }
    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
        self.0.get(name).cloned()
    }
}

fn derive_test_manifest(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    parents: Vec<TestManifestIdU64>,
    changes_str: BTreeMap<&str, Option<&str>>,
) -> impl Future<Output = Result<Option<TestManifestIdU64>>> {
    let changes: Result<_> = (|| {
        let mut changes = Vec::new();
        for (path, change) in changes_str {
            let path = MPath::new(path)?;
            changes.push((path, change.map(|leaf| TestLeaf(leaf.to_string()))));
        }
        Ok(changes)
    })();
    async move {
        derive_manifest(
            ctx.clone(),
            blobstore.clone(),
            parents,
            changes?,
            {
                cloned!(ctx, blobstore);
                move |TreeInfo { subentries, .. }| {
                    let subentries = subentries
                        .into_iter()
                        .map(|(path, (_, id))| (path, id))
                        .collect();
                    cloned!(ctx, blobstore);
                    async move {
                        let id = TestManifestU64(subentries).store(ctx, &blobstore).await?;
                        Ok(((), id))
                    }
                }
            },
            move |leaf_info| {
                cloned!(ctx, blobstore);
                async move {
                    match leaf_info.leaf {
                        None => Err(Error::msg("leaf only conflict")),
                        Some(leaf) => {
                            let id = leaf.store(ctx.clone(), &blobstore).await?;
                            Ok(((), id))
                        }
                    }
                }
            },
        )
        .await
    }
}

struct Files(TestManifestIdU64);

impl Loadable for Files {
    type Value = BTreeMap<MPath, String>;

    fn load<'a, B: Blobstore>(
        &'a self,
        ctx: CoreContext,
        blobstore: &'a B,
    ) -> BoxFuture<'a, Result<Self::Value, LoadableError>> {
        let blobstore = blobstore.clone();
        bounded_traversal_stream(
            256,
            Some((None, Entry::Tree(self.0))),
            move |(path, entry)| {
                cloned!(ctx);
                async move {
                    let content = Loadable::load(&entry, ctx, blobstore).await?;
                    Ok(match content {
                        Entry::Leaf(leaf) => (Some((path, leaf)), Vec::new()),
                        Entry::Tree(tree) => {
                            let recurse = tree
                                .list()
                                .map(|(name, entry)| {
                                    (Some(MPath::join_opt_element(path.as_ref(), &name)), entry)
                                })
                                .collect();
                            (None, recurse)
                        }
                    })
                }
            },
        )
        .try_filter_map(|item| {
            let item = item.and_then(|(path, leaf)| Some((path?, leaf.0)));
            future::ok(item)
        })
        .try_fold(BTreeMap::new(), |mut acc, (path, leaf)| {
            acc.insert(path, leaf);
            future::ok::<_, LoadableError>(acc)
        })
        .boxed()
    }
}

fn files_reference(files: BTreeMap<&str, &str>) -> Result<BTreeMap<MPath, String>, Error> {
    files
        .into_iter()
        .map(|(path, content)| Ok((MPath::new(path)?, content.to_string())))
        .collect()
}

#[test]
fn test_path_tree() -> Result<(), Error> {
    let tree = PathTree::from_iter(vec![
        (MPath::new("/one/two/three")?, true),
        (MPath::new("/one/two/four")?, true),
        (MPath::new("/one/two")?, true),
        (MPath::new("/five")?, true),
    ]);

    let reference = vec![
        (None, false),
        (Some(MPath::new("one")?), false),
        (Some(MPath::new("one/two")?), true),
        (Some(MPath::new("one/two/three")?), true),
        (Some(MPath::new("one/two/four")?), true),
        (Some(MPath::new("five")?), true),
    ];

    assert_eq!(Vec::from_iter(tree), reference);
    Ok(())
}

#[fbinit::test]
async fn test_derive_manifest(fb: FacebookInit) -> Result<(), Error> {
    let blobstore: Arc<dyn Blobstore> = Arc::new(LazyMemblob::default());
    let ctx = CoreContext::test_mock(fb);

    // derive manifest
    let derive = {
        cloned!(ctx, blobstore);
        move |parents, changes| {
            cloned!(ctx, blobstore);
            async move {
                derive_test_manifest(ctx, blobstore, parents, changes)
                    .await
                    .map(|mf| mf.expect("expect non empty manifest"))
            }
        }
    };

    // load all files for specified manifest
    let files = {
        cloned!(ctx, blobstore);
        move |manifest_id| {
            cloned!(ctx, blobstore);
            async move { Loadable::load(&Files(manifest_id), ctx, &blobstore).await }
        }
    };

    // clean merge of two directories
    {
        let mf0 = derive(
            vec![],
            btreemap! {
                "/one/two" => Some("two"),
                "/one/three" => Some("three"),
                "/five/six" => Some("six"),
            },
        )
        .await?;
        let mf1 = derive(
            vec![],
            btreemap! {
                "/one/four" => Some("four"),
                "/one/three" => Some("three"),
                "/five/six" => Some("six"),
            },
        )
        .await?;
        let mf2 = derive(vec![mf0, mf1], btreemap! {}).await?;
        assert_eq!(
            files_reference(btreemap! {
                "/one/two" => "two",     // from mf0
                "/one/three" => "three", // from mf1
                "/one/four" => "four",   // shared leaf mf{0|1}
                "/five/six" => "six",    // shared tree mf{0|1}
            })?,
            files(mf2).await?,
        );
    }

    // remove all files
    {
        let mf0 = derive(
            vec![],
            btreemap! {
                "/one/two" => Some("two"),
                "/one/three" => Some("three"),
                "/five/six" => Some("six"),
            },
        )
        .await?;
        let mf1 = derive_test_manifest(
            ctx.clone(),
            blobstore.clone(),
            vec![mf0],
            btreemap! {
                "/one/two" => None,
                "/one/three" => None,
                "five/six" => None,
            },
        )
        .await?;
        assert!(mf1.is_none(), "empty manifest expected");
    }

    // file-file conflict
    {
        let mf0 = derive(
            vec![],
            btreemap! {
                "/one/two" => Some("conflict"),
                "/three" => Some("not a conflict")
            },
        )
        .await?;
        let mf1 = derive(
            vec![],
            btreemap! {
                "/one/two" => Some("CONFLICT"),
                "/three" => Some("not a conflict"),
            },
        )
        .await?;

        // make sure conflict is dectected
        assert!(
            derive(vec![mf0, mf1], btreemap! {},).await.is_err(),
            "should contain file-file conflict"
        );

        // resolve by overriding file content
        let mf2 = derive(
            vec![mf0, mf1],
            btreemap! {
                "/one/two" => Some("resolved"),
            },
        )
        .await?;
        assert_eq!(
            files_reference(btreemap! {
                "/one/two" => "resolved", // new file replaces conflict
                "/three" => "not a conflict",
            })?,
            files(mf2).await?,
        );

        // resolve by deletion
        let mf3 = derive(
            vec![mf0, mf1],
            btreemap! {
                "/one/two" => None,
            },
        )
        .await?;
        assert_eq!(
            files_reference(btreemap! {
                // file is deleted
                "/three" => "not a conflict",
            })?,
            files(mf3).await?,
        );

        // resolve by overriding directory
        let mf4 = derive(
            vec![mf0, mf1],
            btreemap! {
                "/one" => Some("resolved"),
            },
        )
        .await?;
        assert_eq!(
            files_reference(btreemap! {
                "/one" => "resolved", // whole subdirectory replace by file
                "/three" => "not a conflict",
            })?,
            files(mf4).await?,
        );
    }

    // file-derectory conflict
    {
        let mf0 = derive(
            vec![],
            btreemap! {
                "/one" => Some("file"),
                "/three" => Some("not a conflict")
            },
        )
        .await?;
        let mf1 = derive(
            vec![],
            btreemap! {
                "/one/two" => Some("directory"),
                "/three" => Some("not a conflict"),
            },
        )
        .await?;

        // make sure conflict is dectected
        assert!(
            derive(vec![mf0, mf1], btreemap! {}).await.is_err(),
            "should contain file-file conflict"
        );

        // resolve by deleting file
        let mf2 = derive(
            vec![mf0, mf1],
            btreemap! {
                "/one" => None,
            },
        )
        .await?;
        assert_eq!(
            files_reference(btreemap! {
                "/one/two" => "directory",
                "/three" => "not a conflict",
            })?,
            files(mf2).await?,
        );

        // resolve by deleting of directory
        // NOTE: if you want to resolve file-directory conflict in favour of a file
        //       the only way to do this, is place file with same content. Deletion
        //       of all files in directory is not a valid resolution of a conflict.
        let mf3 = derive(
            vec![mf0, mf1],
            btreemap! {
                "/one" => Some("file"),
            },
        )
        .await?;
        assert_eq!(
            files_reference(btreemap! {
                "/one" => "file",
                "/three" => "not a conflict",
            })?,
            files(mf3).await?,
        );
    }

    Ok(())
}

fn make_paths(paths_str: &[&str]) -> Result<BTreeSet<Option<MPath>>, Error> {
    paths_str
        .into_iter()
        .map(|path_str| match path_str {
            &"/" => Ok(None),
            _ => MPath::new(path_str).map(Some),
        })
        .collect()
}

#[fbinit::test]
async fn test_find_entries(fb: FacebookInit) -> Result<(), Error> {
    let blobstore: Arc<dyn Blobstore> = Arc::new(LazyMemblob::default());
    let ctx = CoreContext::test_mock(fb);

    let mf0 = derive_test_manifest(
        ctx.clone(),
        blobstore.clone(),
        vec![],
        btreemap! {
            "one/1" => Some("1"),
            "one/2" => Some("2"),
            "3" => Some("3"),
            "two/three/4" => Some("4"),
            "two/three/5" => Some("5"),
            "two/three/four/6" => Some("6"),
            "two/three/four/7" => Some("7"),
            "five/six/8" => Some("8"),
            "five/seven/eight/9" => Some("9"),
            "five/seven/eight/nine/10" => Some("10"),
            "five/seven/11" => Some("11"),
        },
    )
    .await?
    .expect("expect non empty manifest");

    // use single select
    {
        let paths = make_paths(&[
            "one/1",
            "two/three",
            "none",
            "two/three/four/7",
            "two/three/6",
        ])?;

        let results: Vec<_> = mf0
            .find_entries(ctx.clone(), blobstore.clone(), paths)
            .try_collect()
            .await?;

        let mut leafs = BTreeSet::new();
        let mut trees = BTreeSet::new();
        for (path, entry) in results {
            match entry {
                Entry::Tree(_) => trees.insert(path),
                Entry::Leaf(_) => leafs.insert(path),
            };
        }

        assert_eq!(leafs, make_paths(&["one/1", "two/three/four/7",])?);
        assert_eq!(trees, make_paths(&["two/three"])?);
    }

    // use prefix + single select
    {
        let paths = vec![
            PathOrPrefix::Path(Some(MPath::new("two/three/5")?)),
            PathOrPrefix::Path(Some(MPath::new("five/seven/11")?)),
            PathOrPrefix::Prefix(Some(MPath::new("five/seven/eight")?)),
        ];

        let results: Vec<_> = mf0
            .find_entries(ctx.clone(), blobstore.clone(), paths)
            .try_collect()
            .await?;

        let mut leafs = BTreeSet::new();
        let mut trees = BTreeSet::new();
        for (path, entry) in results {
            match entry {
                Entry::Tree(_) => trees.insert(path),
                Entry::Leaf(_) => leafs.insert(path),
            };
        }

        assert_eq!(
            leafs,
            make_paths(&[
                "two/three/5",
                "five/seven/11",
                "five/seven/eight/9",
                "five/seven/eight/nine/10"
            ])?
        );
        assert_eq!(
            trees,
            make_paths(&["five/seven/eight", "five/seven/eight/nine"])?
        );
    }

    // find entry
    {
        let mf_root = mf0.find_entry(ctx.clone(), blobstore.clone(), None).await?;
        assert_eq!(mf_root, Some(Entry::Tree(mf0)));

        let paths = make_paths(&[
            "one/1",
            "two/three",
            "none",
            "two/three/four/7",
            "two/three/6",
            "five/seven/11",
        ])?;

        let mut leafs = BTreeSet::new();
        let mut trees = BTreeSet::new();
        for path in paths {
            let entry = mf0
                .find_entry(ctx.clone(), blobstore.clone(), path.clone())
                .await?;
            match entry {
                Some(Entry::Tree(_)) => {
                    trees.insert(path);
                }
                Some(Entry::Leaf(_)) => {
                    leafs.insert(path);
                }
                _ => {}
            };
        }

        assert_eq!(
            leafs,
            make_paths(&["one/1", "two/three/four/7", "five/seven/11"])?
        );
        assert_eq!(trees, make_paths(&["two/three"])?);
    }

    Ok(())
}

#[fbinit::test]
async fn test_diff(fb: FacebookInit) -> Result<(), Error> {
    let blobstore: Arc<dyn Blobstore> = Arc::new(LazyMemblob::default());
    let ctx = CoreContext::test_mock(fb);

    let mf0 = derive_test_manifest(
        ctx.clone(),
        blobstore.clone(),
        vec![],
        btreemap! {
            "same_dir/1" => Some("1"),
            "dir/same_dir/2" => Some("2"),
            "dir/removed_dir/3" => Some("3"),
            "dir/changed_file" => Some("before"),
            "dir/removed_file" => Some("removed_file"),
            "same_file" => Some("4"),
            "dir_file_conflict/5" => Some("5"),
        },
    )
    .await?
    .expect("expect non empty manifest");

    let mf1 = derive_test_manifest(
        ctx.clone(),
        blobstore.clone(),
        vec![],
        btreemap! {
            "same_dir/1" => Some("1"),
            "dir/same_dir/2" => Some("2"),
            "dir/added_dir/3" => Some("3"),
            "dir/changed_file" => Some("after"),
            "added_file" => Some("added_file"),
            "same_file" => Some("4"),
            "dir_file_conflict" => Some("5"),
        },
    )
    .await?
    .expect("expect non empty manifest");

    let diffs: Vec<_> = mf0
        .diff(ctx.clone(), blobstore.clone(), mf1)
        .try_collect()
        .await?;

    let mut added = BTreeSet::new();
    let mut removed = BTreeSet::new();
    let mut changed = BTreeSet::new();
    for diff in diffs {
        match diff {
            Diff::Added(path, _) => {
                added.insert(path);
            }
            Diff::Removed(path, _) => {
                removed.insert(path);
            }
            Diff::Changed(path, _, _) => {
                changed.insert(path);
            }
        };
    }

    assert_eq!(
        added,
        make_paths(&[
            "added_file",
            "dir/added_dir",
            "dir/added_dir/3",
            "dir_file_conflict"
        ])?
    );
    assert_eq!(
        removed,
        make_paths(&[
            "dir/removed_file",
            "dir/removed_dir",
            "dir/removed_dir/3",
            "dir_file_conflict",
            "dir_file_conflict/5"
        ])?
    );
    assert_eq!(changed, make_paths(&["/", "dir", "dir/changed_file"])?);

    let diffs: Vec<_> = mf0
        .filtered_diff(
            ctx,
            blobstore,
            mf1,
            |diff| {
                let path = match &diff {
                    Diff::Added(path, ..) | Diff::Removed(path, ..) | Diff::Changed(path, ..) => {
                        path
                    }
                };

                let badpath = MPath::new("dir/added_dir/3").unwrap();
                if &Some(badpath) != path {
                    Some(diff)
                } else {
                    None
                }
            },
            |diff| {
                let path = match diff {
                    Diff::Added(path, ..) | Diff::Removed(path, ..) | Diff::Changed(path, ..) => {
                        path
                    }
                };

                let badpath = MPath::new("dir/removed_dir").unwrap();
                &Some(badpath) != path
            },
        )
        .try_collect()
        .await?;

    let mut added = BTreeSet::new();
    let mut removed = BTreeSet::new();
    let mut changed = BTreeSet::new();
    for diff in diffs {
        match diff {
            Diff::Added(path, _) => {
                added.insert(path);
            }
            Diff::Removed(path, _) => {
                removed.insert(path);
            }
            Diff::Changed(path, _, _) => {
                changed.insert(path);
            }
        };
    }

    assert_eq!(
        added,
        make_paths(&["added_file", "dir/added_dir", "dir_file_conflict"])?
    );
    assert_eq!(
        removed,
        make_paths(&[
            "dir/removed_file",
            "dir_file_conflict",
            "dir_file_conflict/5"
        ])?
    );
    assert_eq!(changed, make_paths(&["/", "dir", "dir/changed_file"])?);

    Ok(())
}

#[fbinit::test]
async fn test_find_intersection_of_diffs(fb: FacebookInit) -> Result<(), Error> {
    let blobstore: Arc<dyn Blobstore> = Arc::new(LazyMemblob::default());
    let ctx = CoreContext::test_mock(fb);

    let mf0 = derive_test_manifest(
        ctx.clone(),
        blobstore.clone(),
        vec![],
        btreemap! {
            "same_dir/1" => Some("1"),
            "dir/same_dir/2" => Some("2"),
            "dir/removed_dir/3" => Some("3"),
            "dir/changed_file" => Some("before"),
            "dir/removed_file" => Some("removed_file"),
            "same_file" => Some("4"),
            "dir_file_conflict/5" => Some("5"),
        },
    )
    .await?
    .expect("expect non empty manifest");

    let mf1 = derive_test_manifest(
        ctx.clone(),
        blobstore.clone(),
        vec![],
        btreemap! {
            "same_dir/1" => Some("1"),
            "dir/same_dir/2" => Some("2"),
            "dir/added_dir/3" => Some("3"),
            "dir/changed_file" => Some("after"),
            "added_file" => Some("added_file"),
            "same_file" => Some("4"),
            "dir_file_conflict" => Some("5"),
        },
    )
    .await?
    .expect("expect non empty manifest");

    let mf2 = derive_test_manifest(
        ctx.clone(),
        blobstore.clone(),
        vec![],
        btreemap! {
            "added_file" => Some("added_file"),
            "same_file" => Some("4"),
            "dir_file_conflict" => Some("5"),
        },
    )
    .await?
    .expect("expect non empty manifest");

    let intersection: Vec<_> =
        find_intersection_of_diffs(ctx.clone(), blobstore.clone(), mf0, vec![mf0])
            .try_collect()
            .await?;

    assert_eq!(intersection, vec![]);

    let intersection: Vec<_> =
        find_intersection_of_diffs(ctx.clone(), blobstore.clone(), mf1, vec![mf0])
            .try_collect()
            .await?;

    let intersection: BTreeSet<_> = intersection.into_iter().map(|(path, _)| path).collect();

    assert_eq!(
        intersection,
        make_paths(&[
            "/",
            "added_file",
            "dir",
            "dir/added_dir",
            "dir/added_dir/3",
            "dir/changed_file",
            "dir_file_conflict"
        ])?,
    );

    // Diff against two manifests
    let intersection: Vec<_> = find_intersection_of_diffs(ctx, blobstore, mf1, vec![mf0, mf2])
        .try_collect()
        .await?;

    let intersection: BTreeSet<_> = intersection.into_iter().map(|(path, _)| path).collect();

    assert_eq!(
        intersection,
        make_paths(&[
            "/",
            "dir",
            "dir/added_dir",
            "dir/added_dir/3",
            "dir/changed_file",
        ])?,
    );

    Ok(())
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub(crate) struct TestManifestIdStr(pub &'static str);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub(crate) struct TestFileId(pub &'static str);

#[derive(Default, Clone, Debug)]
pub(crate) struct TestManifestStr(
    pub HashMap<MPathElement, BonsaiEntry<TestManifestIdStr, TestFileId>>,
);

#[derive(Default, Debug, Clone)]
pub(crate) struct ManifestStore(pub HashMap<TestManifestIdStr, TestManifestStr>);

impl StoreLoadable<ManifestStore> for TestManifestIdStr {
    type Value = TestManifestStr;

    fn load(
        &self,
        _ctx: CoreContext,
        store: &ManifestStore,
    ) -> BoxFuture<'static, Result<Self::Value, LoadableError>> {
        let value = store
            .0
            .get(&self)
            .cloned()
            .ok_or(LoadableError::Missing(format!("missing {}", self.0)));
        async move { value }.boxed()
    }
}

impl Manifest for TestManifestStr {
    type TreeId = TestManifestIdStr;
    type LeafId = (FileType, TestFileId);

    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
        self.0.get(name).cloned()
    }

    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
        Box::new(self.0.clone().into_iter())
    }
}

pub(crate) fn ctx(fb: FacebookInit) -> CoreContext {
    CoreContext::test_mock(fb)
}

pub(crate) fn element(s: &str) -> MPathElement {
    MPathElement::new(s.as_bytes().iter().cloned().collect()).unwrap()
}

pub(crate) fn path(s: &str) -> MPath {
    MPath::new(s).unwrap()
}

pub(crate) fn file(ty: FileType, name: &'static str) -> BonsaiEntry<TestManifestIdStr, TestFileId> {
    BonsaiEntry::Leaf((ty, TestFileId(name)))
}

pub(crate) fn dir(name: &'static str) -> BonsaiEntry<TestManifestIdStr, TestFileId> {
    BonsaiEntry::Tree(TestManifestIdStr(name))
}

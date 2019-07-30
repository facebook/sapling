// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::{derive_manifest, Diff, Entry, Manifest, ManifestOps, PathTree, TreeInfo};
use blobstore::{Blobstore, BlobstoreBytes, Loadable, Storable};
use context::CoreContext;
use failure::{err_msg, Error};
use futures::{future, Future, IntoFuture, Stream};
use futures_ext::{bounded_traversal::bounded_traversal_stream, BoxFuture, FutureExt};
use lock_ext::LockExt;
use maplit::btreemap;
use memblob::LazyMemblob;
use mononoke_types::{MPath, MPathElement};
use pretty_assertions::assert_eq;
use serde_derive::{Deserialize, Serialize};
use std::{
    collections::{hash_map::DefaultHasher, BTreeMap, HashSet},
    hash::{Hash, Hasher},
    iter::FromIterator,
    sync::{Arc, Mutex},
};
use tokio::runtime::Runtime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct TestLeafId(u64);

#[derive(Debug)]
struct TestLeaf(String);

impl Loadable for TestLeafId {
    type Value = TestLeaf;

    fn load(
        &self,
        ctx: CoreContext,
        blobstore: impl Blobstore + Clone,
    ) -> BoxFuture<Self::Value, Error> {
        blobstore
            .get(ctx, self.0.to_string())
            .and_then(|bytes| {
                let bytes = bytes.ok_or_else(|| err_msg("failed to load a leaf"))?;
                Ok(TestLeaf(std::str::from_utf8(bytes.as_bytes())?.to_owned()))
            })
            .boxify()
    }
}

impl Storable for TestLeaf {
    type Key = TestLeafId;

    fn store(
        &self,
        ctx: CoreContext,
        blobstore: impl Blobstore + Clone,
    ) -> BoxFuture<Self::Key, Error> {
        let mut hasher = DefaultHasher::new();
        self.0.hash(&mut hasher);
        let key = TestLeafId(hasher.finish());
        blobstore
            .put(
                ctx,
                key.0.to_string(),
                BlobstoreBytes::from_bytes(self.0.clone()),
            )
            .map(move |_| key)
            .boxify()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct TestManifestId(u64);

#[derive(Debug, Serialize, Deserialize)]
struct TestManifest(BTreeMap<MPathElement, Entry<TestManifestId, TestLeafId>>);

impl Loadable for TestManifestId {
    type Value = TestManifest;

    fn load(
        &self,
        ctx: CoreContext,
        blobstore: impl Blobstore + Clone,
    ) -> BoxFuture<Self::Value, Error> {
        blobstore
            .get(ctx, self.0.to_string())
            .and_then(|data| data.ok_or_else(|| err_msg("failed to load a manifest")))
            .and_then(|bytes| Ok(serde_cbor::from_slice(bytes.as_bytes().as_ref())?))
            .boxify()
    }
}

impl Storable for TestManifest {
    type Key = TestManifestId;

    fn store(
        &self,
        ctx: CoreContext,
        blobstore: impl Blobstore + Clone,
    ) -> BoxFuture<Self::Key, Error> {
        let mut hasher = DefaultHasher::new();
        self.0.hash(&mut hasher);
        let key = TestManifestId(hasher.finish());
        serde_cbor::to_vec(self)
            .into_future()
            .from_err()
            .and_then(move |bytes| {
                blobstore.put(ctx, key.0.to_string(), BlobstoreBytes::from_bytes(bytes))
            })
            .map(move |_| key)
            .boxify()
    }
}

impl Manifest for TestManifest {
    type LeafId = TestLeafId;
    type TreeId = TestManifestId;

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
    parents: Vec<TestManifestId>,
    changes: BTreeMap<&str, Option<&str>>,
) -> impl Future<Item = Option<TestManifestId>, Error = Error> {
    let changes: Result<Vec<_>, _> = changes
        .iter()
        .map(|(path, change)| {
            MPath::new(path).map(|path| (path, change.map(|leaf| TestLeaf(leaf.to_owned()))))
        })
        .collect();
    changes.into_future().and_then(move |changes| {
        derive_manifest(
            ctx.clone(),
            blobstore.clone(),
            parents,
            changes,
            {
                let ctx = ctx.clone();
                let blobstore = blobstore.clone();
                move |TreeInfo { subentries, .. }| {
                    TestManifest(subentries).store(ctx.clone(), blobstore.clone())
                }
            },
            move |leaf_info| match leaf_info.leaf {
                None => future::err(err_msg("leaf only conflict")).left_future(),
                Some(leaf) => leaf.store(ctx.clone(), blobstore.clone()).right_future(),
            },
        )
    })
}

struct Files(TestManifestId);

impl Loadable for Files {
    type Value = BTreeMap<MPath, String>;

    fn load(
        &self,
        ctx: CoreContext,
        blobstore: impl Blobstore + Clone,
    ) -> BoxFuture<Self::Value, Error> {
        bounded_traversal_stream(256, (None, Entry::Tree(self.0)), move |(path, entry)| {
            entry
                .load(ctx.clone(), blobstore.clone())
                .map(move |content| match content {
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
        })
        .filter_map(|item| {
            let (path, leaf) = item?;
            Some((path?, leaf.0))
        })
        .fold(BTreeMap::new(), |mut acc, (path, leaf)| {
            acc.insert(path, leaf);
            Ok::<_, Error>(acc)
        })
        .boxify()
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

#[test]
fn test_derive_manifest() -> Result<(), Error> {
    let runtime = Arc::new(Mutex::new(Runtime::new()?));
    let blobstore: Arc<dyn Blobstore> = Arc::new(LazyMemblob::new());
    let ctx = CoreContext::test_mock();

    // derive manifest
    let derive = |parents, changes| -> Result<TestManifestId, Error> {
        let manifest_id = runtime
            .with(|runtime| {
                runtime.block_on(derive_test_manifest(
                    ctx.clone(),
                    blobstore.clone(),
                    parents,
                    changes,
                ))
            })?
            .expect("expect non empty manifest");
        Ok(manifest_id)
    };

    // load all files for specified manifest
    let files = |manifest_id| {
        runtime.with(|runtime| {
            runtime.block_on(Files(manifest_id).load(ctx.clone(), blobstore.clone()))
        })
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
        )?;
        let mf1 = derive(
            vec![],
            btreemap! {
                "/one/four" => Some("four"),
                "/one/three" => Some("three"),
                "/five/six" => Some("six"),
            },
        )?;
        let mf2 = derive(vec![mf0, mf1], btreemap! {})?;
        assert_eq!(
            files_reference(btreemap! {
                "/one/two" => "two",     // from mf0
                "/one/three" => "three", // from mf1
                "/one/four" => "four",   // shared leaf mf{0|1}
                "/five/six" => "six",    // shared tree mf{0|1}
            })?,
            files(mf2)?,
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
        )?;
        let mf1 = runtime.with(|runtime| {
            runtime.block_on(derive_test_manifest(
                ctx.clone(),
                blobstore.clone(),
                vec![mf0],
                btreemap! {
                    "/one/two" => None,
                    "/one/three" => None,
                    "five/six" => None,
                },
            ))
        })?;
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
        )?;
        let mf1 = derive(
            vec![],
            btreemap! {
                "/one/two" => Some("CONFLICT"),
                "/three" => Some("not a conflict"),
            },
        )?;

        // make sure conflict is dectected
        assert!(
            derive(vec![mf0, mf1], btreemap! {},).is_err(),
            "should contain file-file conflict"
        );

        // resolve by overriding file content
        let mf2 = derive(
            vec![mf0, mf1],
            btreemap! {
                "/one/two" => Some("resolved"),
            },
        )?;
        assert_eq!(
            files_reference(btreemap! {
                "/one/two" => "resolved", // new file replaces conflict
                "/three" => "not a conflict",
            })?,
            files(mf2)?,
        );

        // resolve by deletion
        let mf3 = derive(
            vec![mf0, mf1],
            btreemap! {
                "/one/two" => None,
            },
        )?;
        assert_eq!(
            files_reference(btreemap! {
                // file is deleted
                "/three" => "not a conflict",
            })?,
            files(mf3)?,
        );

        // resolve by overriding directory
        let mf4 = derive(
            vec![mf0, mf1],
            btreemap! {
                "/one" => Some("resolved"),
            },
        )?;
        assert_eq!(
            files_reference(btreemap! {
                "/one" => "resolved", // whole subdirectory replace by file
                "/three" => "not a conflict",
            })?,
            files(mf4)?,
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
        )?;
        let mf1 = derive(
            vec![],
            btreemap! {
                "/one/two" => Some("directory"),
                "/three" => Some("not a conflict"),
            },
        )?;

        // make sure conflict is dectected
        assert!(
            derive(vec![mf0, mf1], btreemap! {}).is_err(),
            "should contain file-file conflict"
        );

        // resolve by deleting file
        let mf2 = derive(
            vec![mf0, mf1],
            btreemap! {
                "/one" => None,
            },
        )?;
        assert_eq!(
            files_reference(btreemap! {
                "/one/two" => "directory",
                "/three" => "not a conflict",
            })?,
            files(mf2)?,
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
        )?;
        assert_eq!(
            files_reference(btreemap! {
                "/one" => "file",
                "/three" => "not a conflict",
            })?,
            files(mf3)?,
        );
    }

    Ok(())
}

fn make_paths(paths_str: &[&str]) -> Result<HashSet<MPath>, Error> {
    paths_str.into_iter().map(MPath::new).collect()
}

#[test]
fn test_find_entries() -> Result<(), Error> {
    let runtime = Arc::new(Mutex::new(Runtime::new()?));
    let blobstore: Arc<dyn Blobstore> = Arc::new(LazyMemblob::new());
    let ctx = CoreContext::test_mock();

    // derive manifest
    let derive = |parents, changes| -> Result<TestManifestId, Error> {
        let manifest_id = runtime
            .with(|runtime| {
                runtime.block_on(derive_test_manifest(
                    ctx.clone(),
                    blobstore.clone(),
                    parents,
                    changes,
                ))
            })?
            .expect("expect non empty manifest");
        Ok(manifest_id)
    };

    let mf0 = derive(
        vec![],
        btreemap! {
            "one/1" => Some("1"),
            "one/2" => Some("2"),
            "3" => Some("3"),
            "two/three/4" => Some("4"),
            "two/three/5" => Some("5"),
            "two/three/four/6" => Some("6"),
            "two/three/four/7" => Some("7"),
        },
    )?;

    let paths = make_paths(&[
        "one/1",
        "two/three",
        "none",
        "two/three/four/7",
        "two/three/6",
    ])?;

    let results =
        runtime.with(|rt| rt.block_on(mf0.find_entries(ctx, blobstore, paths).collect()))?;

    let mut leafs = HashSet::new();
    let mut trees = HashSet::new();
    for (path, entry) in results {
        match entry {
            Entry::Tree(_) => trees.insert(path),
            Entry::Leaf(_) => leafs.insert(path),
        };
    }

    assert_eq!(leafs, make_paths(&["one/1", "two/three/four/7",])?);
    assert_eq!(trees, make_paths(&["two/three"])?);

    Ok(())
}

#[test]
fn test_diff() -> Result<(), Error> {
    let runtime = Arc::new(Mutex::new(Runtime::new()?));
    let blobstore: Arc<dyn Blobstore> = Arc::new(LazyMemblob::new());
    let ctx = CoreContext::test_mock();

    // derive manifest
    let derive = |parents, changes| -> Result<TestManifestId, Error> {
        let manifest_id = runtime
            .with(|runtime| {
                runtime.block_on(derive_test_manifest(
                    ctx.clone(),
                    blobstore.clone(),
                    parents,
                    changes,
                ))
            })?
            .expect("expect non empty manifest");
        Ok(manifest_id)
    };

    let mf0 = derive(
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
    )?;

    let mf1 = derive(
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
    )?;

    let diffs = runtime.with(|rt| rt.block_on(mf0.diff(ctx, blobstore, mf1).collect()))?;

    let mut added = HashSet::new();
    let mut removed = HashSet::new();
    let mut changed = HashSet::new();
    for diff in diffs {
        match diff {
            Diff::Added(Some(path), _) => {
                added.insert(path);
            }
            Diff::Removed(Some(path), _) => {
                removed.insert(path);
            }
            Diff::Changed(Some(path), _, _) => {
                changed.insert(path);
            }
            _ => {}
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
    assert_eq!(changed, make_paths(&["dir", "dir/changed_file"])?);

    Ok(())
}

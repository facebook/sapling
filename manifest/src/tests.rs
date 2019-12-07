/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::{
    derive_manifest, find_intersection_of_diffs, Diff, Entry, Manifest, ManifestOps, PathOrPrefix,
    PathTree, TreeInfo,
};
use anyhow::Error;
use blobstore::{Blobstore, Loadable, LoadableError, Storable};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{future, stream, Future, IntoFuture, Stream};
use futures_ext::{bounded_traversal::bounded_traversal_stream, BoxFuture, FutureExt};
use lock_ext::LockExt;
use maplit::btreemap;
use memblob::LazyMemblob;
use mononoke_types::{BlobstoreBytes, MPath, MPathElement};
use pretty_assertions::assert_eq;
use serde_derive::{Deserialize, Serialize};
use std::{
    collections::{hash_map::DefaultHasher, BTreeMap, BTreeSet},
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

    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Value, LoadableError> {
        let key = self.0.to_string();
        blobstore
            .get(ctx, key.clone())
            .from_err()
            .and_then(move |bytes| {
                let bytes = bytes.ok_or(LoadableError::Missing(key))?;
                let bytes = std::str::from_utf8(bytes.as_bytes())
                    .map_err(|err| LoadableError::Error(Error::from(err)))?;
                Ok(TestLeaf(bytes.to_owned()))
            })
            .boxify()
    }
}

impl Storable for TestLeaf {
    type Key = TestLeafId;

    fn store<B: Blobstore + Clone>(
        self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Key, Error> {
        let mut hasher = DefaultHasher::new();
        self.0.hash(&mut hasher);
        let key = TestLeafId(hasher.finish());
        blobstore
            .put(ctx, key.0.to_string(), BlobstoreBytes::from_bytes(self.0))
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

    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Value, LoadableError> {
        let key = self.0.to_string();
        blobstore
            .get(ctx, key.clone())
            .from_err()
            .and_then(move |data| data.ok_or(LoadableError::Missing(key)))
            .and_then(|bytes| {
                let mf = serde_cbor::from_slice(bytes.as_bytes().as_ref())
                    .map_err(|err| LoadableError::Error(Error::from(err)))?;
                Ok(mf)
            })
            .boxify()
    }
}

impl Storable for TestManifest {
    type Key = TestManifestId;

    fn store<B: Blobstore + Clone>(
        self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Key, Error> {
        let blobstore = blobstore.clone();
        let mut hasher = DefaultHasher::new();
        self.0.hash(&mut hasher);
        let key = TestManifestId(hasher.finish());
        serde_cbor::to_vec(&self)
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
                    let subentries = subentries
                        .into_iter()
                        .map(|(path, (_, id))| (path, id))
                        .collect();
                    TestManifest(subentries)
                        .store(ctx.clone(), &blobstore)
                        .map(|id| ((), id))
                }
            },
            move |leaf_info| match leaf_info.leaf {
                None => future::err(Error::msg("leaf only conflict")).left_future(),
                Some(leaf) => leaf
                    .store(ctx.clone(), &blobstore)
                    .map(|id| ((), id))
                    .right_future(),
            },
        )
    })
}

struct Files(TestManifestId);

impl Loadable for Files {
    type Value = BTreeMap<MPath, String>;

    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Value, LoadableError> {
        let blobstore = blobstore.clone();

        bounded_traversal_stream(
            256,
            Some((None, Entry::Tree(self.0))),
            move |(path, entry)| {
                entry
                    .load(ctx.clone(), &blobstore)
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
            },
        )
        .filter_map(|item| {
            let (path, leaf) = item?;
            Some((path?, leaf.0))
        })
        .fold(BTreeMap::new(), |mut acc, (path, leaf)| {
            acc.insert(path, leaf);
            Ok::<_, LoadableError>(acc)
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

#[fbinit::test]
fn test_derive_manifest(fb: FacebookInit) -> Result<(), Error> {
    let runtime = Arc::new(Mutex::new(Runtime::new()?));
    let blobstore: Arc<dyn Blobstore> = Arc::new(LazyMemblob::new());
    let ctx = CoreContext::test_mock(fb);

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
        runtime.with(|runtime| runtime.block_on(Files(manifest_id).load(ctx.clone(), &blobstore)))
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
fn test_find_entries(fb: FacebookInit) -> Result<(), Error> {
    let rt = Arc::new(Mutex::new(Runtime::new()?));
    let blobstore: Arc<dyn Blobstore> = Arc::new(LazyMemblob::new());
    let ctx = CoreContext::test_mock(fb);

    // derive manifest
    let derive = |parents, changes| -> Result<TestManifestId, Error> {
        let manifest_id = rt
            .with(|rt| {
                rt.block_on(derive_test_manifest(
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
            "five/six/8" => Some("8"),
            "five/seven/eight/9" => Some("9"),
            "five/seven/eight/nine/10" => Some("10"),
            "five/seven/11" => Some("11"),
        },
    )?;

    // use single select
    {
        let paths = make_paths(&[
            "one/1",
            "two/three",
            "none",
            "two/three/four/7",
            "two/three/6",
        ])?;

        let results = rt.with(|rt| {
            rt.block_on(
                mf0.find_entries(ctx.clone(), blobstore.clone(), paths)
                    .collect(),
            )
        })?;

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

        let results = rt.with(|rt| {
            rt.block_on(
                mf0.find_entries(ctx.clone(), blobstore.clone(), paths)
                    .collect(),
            )
        })?;

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
        let mf_root =
            rt.with(|rt| rt.block_on(mf0.find_entry(ctx.clone(), blobstore.clone(), None)))?;
        assert_eq!(mf_root, Some(Entry::Tree(mf0)));

        let paths = make_paths(&[
            "one/1",
            "two/three",
            "none",
            "two/three/four/7",
            "two/three/6",
            "five/seven/11",
        ])?;

        let results = rt.with(|rt| {
            rt.block_on(
                stream::iter_ok(paths)
                    .and_then(move |path| {
                        mf0.find_entry(ctx.clone(), blobstore.clone(), path.clone())
                            .map(move |entry| entry.map(|entry| (path, entry)))
                    })
                    .collect(),
            )
        })?;

        let mut leafs = BTreeSet::new();
        let mut trees = BTreeSet::new();
        for (path, entry) in results.into_iter().flatten() {
            match entry {
                Entry::Tree(_) => trees.insert(path),
                Entry::Leaf(_) => leafs.insert(path),
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
fn test_diff(fb: FacebookInit) -> Result<(), Error> {
    let runtime = Arc::new(Mutex::new(Runtime::new()?));
    let blobstore: Arc<dyn Blobstore> = Arc::new(LazyMemblob::new());
    let ctx = CoreContext::test_mock(fb);

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

    let diffs =
        runtime.with(|rt| rt.block_on(mf0.diff(ctx.clone(), blobstore.clone(), mf1).collect()))?;

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

    let diffs = runtime.with(|rt| {
        rt.block_on(
            mf0.filtered_diff(
                ctx,
                blobstore,
                mf1,
                |diff| {
                    let path = match &diff {
                        Diff::Added(path, ..)
                        | Diff::Removed(path, ..)
                        | Diff::Changed(path, ..) => path,
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
                        Diff::Added(path, ..)
                        | Diff::Removed(path, ..)
                        | Diff::Changed(path, ..) => path,
                    };

                    let badpath = MPath::new("dir/removed_dir").unwrap();
                    &Some(badpath) != path
                },
            )
            .collect(),
        )
    })?;

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
fn test_find_intersection_of_diffs(fb: FacebookInit) -> Result<(), Error> {
    let runtime = Arc::new(Mutex::new(Runtime::new()?));
    let blobstore: Arc<dyn Blobstore> = Arc::new(LazyMemblob::new());
    let ctx = CoreContext::test_mock(fb);

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

    let mf2 = derive(
        vec![],
        btreemap! {
            "added_file" => Some("added_file"),
            "same_file" => Some("4"),
            "dir_file_conflict" => Some("5"),
        },
    )?;

    let intersection = runtime.with(|rt| {
        rt.block_on(
            find_intersection_of_diffs(ctx.clone(), blobstore.clone(), mf0, vec![mf0]).collect(),
        )
    })?;

    assert_eq!(intersection, vec![]);

    let intersection = runtime.with(|rt| {
        rt.block_on(
            find_intersection_of_diffs(ctx.clone(), blobstore.clone(), mf1, vec![mf0]).collect(),
        )
    })?;

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
    let intersection = runtime.with(|rt| {
        rt.block_on(find_intersection_of_diffs(ctx, blobstore, mf1, vec![mf0, mf2]).collect())
    })?;

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

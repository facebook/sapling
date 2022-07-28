/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub(crate) use crate::bonsai::BonsaiEntry;
pub(crate) use crate::derive_batch::derive_manifests_for_simple_stack_of_commits;
pub(crate) use crate::derive_batch::ManifestChanges;
pub(crate) use crate::derive_manifest;
pub(crate) use crate::find_intersection_of_diffs;
pub(crate) use crate::Diff;
pub(crate) use crate::Entry;
pub(crate) use crate::Manifest;
pub(crate) use crate::ManifestOps;
pub(crate) use crate::ManifestOrderedOps;
pub(crate) use crate::OrderedManifest;
pub(crate) use crate::PathOrPrefix;
pub(crate) use crate::PathTree;
pub(crate) use crate::TreeInfo;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use blobstore::Storable;
use blobstore::StoreLoadable;
use borrowed::borrowed;
use bounded_traversal::bounded_traversal_stream;
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future;
use futures::future::FutureExt;
use futures::stream::TryStreamExt;
use maplit::btreemap;
use memblob::Memblob;
use mononoke_types::BlobstoreBytes;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use mononoke_types_mocks::changesetid::ONES_CSID;
use mononoke_types_mocks::changesetid::THREES_CSID;
use mononoke_types_mocks::changesetid::TWOS_CSID;
use pretty_assertions::assert_eq;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct TestLeafId(u64);

#[derive(Debug)]
struct TestLeaf(String);

#[async_trait]
impl Loadable for TestLeafId {
    type Value = TestLeaf;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        let key = self.0.to_string();
        let bytes = blobstore
            .get(ctx, &key)
            .await?
            .ok_or(LoadableError::Missing(key))?;
        let bytes = std::str::from_utf8(bytes.as_raw_bytes())
            .map_err(|err| LoadableError::Error(Error::from(err)))?;
        Ok(TestLeaf(bytes.to_owned()))
    }
}

#[async_trait]
impl Storable for TestLeaf {
    type Key = TestLeafId;

    async fn store<'a, B: Blobstore>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Key> {
        let mut hasher = DefaultHasher::new();
        self.0.hash(&mut hasher);
        let key = TestLeafId(hasher.finish());
        blobstore
            .put(ctx, key.0.to_string(), BlobstoreBytes::from_bytes(self.0))
            .await?;
        Ok(key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct TestManifestIdU64(u64);

#[derive(Debug, Serialize, Deserialize)]
struct TestManifestU64(BTreeMap<MPathElement, Entry<TestManifestIdU64, TestLeafId>>);

#[async_trait]
impl Loadable for TestManifestIdU64 {
    type Value = TestManifestU64;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        let key = self.0.to_string();
        let get = blobstore.get(ctx, &key);
        let data = get.await?;
        let bytes = data.ok_or(LoadableError::Missing(key))?;
        let mf = serde_cbor::from_slice(bytes.as_raw_bytes().as_ref())
            .map_err(|err| LoadableError::Error(Error::from(err)))?;
        Ok(mf)
    }
}

#[async_trait]
impl Storable for TestManifestU64 {
    type Key = TestManifestIdU64;

    async fn store<'a, B: Blobstore>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Key> {
        let mut hasher = DefaultHasher::new();
        self.0.hash(&mut hasher);
        let key = TestManifestIdU64(hasher.finish());

        let bytes = serde_cbor::to_vec(&self)?;
        blobstore
            .put(ctx, key.0.to_string(), BlobstoreBytes::from_bytes(bytes))
            .await?;
        Ok(key)
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

// Implement OrderedManifests for tests.  Ideally we'd need to know the
// descendant count, but since we don't, just make something up (10).  The
// code still works if this estimate is wrong, but it may schedule the
// wrong number of child manifest expansions when this happens.
impl OrderedManifest for TestManifestU64 {
    fn list_weighted(
        &self,
    ) -> Box<dyn Iterator<Item = (MPathElement, Entry<(usize, Self::TreeId), Self::LeafId>)>> {
        let iter = self.0.clone().into_iter().map(|entry| match entry {
            (elem, Entry::Leaf(leaf)) => (elem, Entry::Leaf(leaf)),
            (elem, Entry::Tree(tree)) => (elem, Entry::Tree((10, tree))),
        });
        Box::new(iter)
    }
    fn lookup_weighted(
        &self,
        name: &MPathElement,
    ) -> Option<Entry<(usize, Self::TreeId), Self::LeafId>> {
        self.0.get(name).cloned().map(|entry| match entry {
            Entry::Leaf(leaf) => Entry::Leaf(leaf),
            Entry::Tree(tree) => Entry::Tree((10, tree)),
        })
    }
}

async fn derive_test_manifest(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    parents: Vec<TestManifestIdU64>,
    changes_str: BTreeMap<&str, Option<&str>>,
) -> Result<Option<TestManifestIdU64>> {
    let changes: Result<_> = (|| {
        let mut changes = Vec::new();
        for (path, change) in changes_str {
            let path = MPath::new(path)?;
            changes.push((path, change.map(|leaf| TestLeaf(leaf.to_string()))));
        }
        Ok(changes)
    })();
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
                    let id = TestManifestU64(subentries).store(&ctx, &blobstore).await?;
                    Ok(((), id))
                }
            }
        },
        {
            cloned!(ctx, blobstore);
            move |leaf_info| {
                cloned!(ctx, blobstore);
                async move {
                    match leaf_info.leaf {
                        None => Err(Error::msg("leaf only conflict")),
                        Some(leaf) => {
                            let id = leaf.store(&ctx, &blobstore).await?;
                            Ok(((), id))
                        }
                    }
                }
            }
        },
    )
    .await
}

async fn derive_test_stack_of_manifests(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    parent: Option<TestManifestIdU64>,
    changes_str_per_cs_id: BTreeMap<ChangesetId, BTreeMap<&str, Option<&str>>>,
) -> Result<BTreeMap<ChangesetId, TestManifestIdU64>> {
    let mut stack_of_commits = vec![];
    let mut all_mf_changes = vec![];
    for (cs_id, changes_str) in changes_str_per_cs_id {
        let mut changes = Vec::new();
        for (path, change) in changes_str {
            let path = MPath::new(path)?;
            changes.push((path, change.map(|leaf| TestLeaf(leaf.to_string()))));
        }
        let mf_changes = ManifestChanges { cs_id, changes };
        all_mf_changes.push(mf_changes);
        stack_of_commits.push(cs_id);
    }

    let derived = derive_manifests_for_simple_stack_of_commits(
        ctx.clone(),
        blobstore.clone(),
        parent,
        all_mf_changes,
        {
            cloned!(ctx, blobstore);
            move |TreeInfo { subentries, .. }, _| {
                let subentries = subentries
                    .into_iter()
                    .map(|(path, (_, id))| (path, id))
                    .collect();
                cloned!(ctx, blobstore);
                async move {
                    let id = TestManifestU64(subentries).store(&ctx, &blobstore).await?;
                    Ok(((), id))
                }
            }
        },
        {
            cloned!(ctx, blobstore);
            move |leaf_info, _| {
                cloned!(ctx, blobstore);
                async move {
                    match leaf_info.leaf {
                        None => Err(Error::msg("leaf only conflict")),
                        Some(leaf) => {
                            let id = leaf.store(&ctx, &blobstore).await?;
                            Ok(((), id))
                        }
                    }
                }
            }
        },
    )
    .await?;

    let mut res = BTreeMap::new();
    for cs_id in stack_of_commits {
        res.insert(cs_id, derived.get(&cs_id).unwrap().clone());
    }
    Ok(res)
}

struct Files(TestManifestIdU64);

#[async_trait]
impl Loadable for Files {
    type Value = BTreeMap<MPath, String>;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        bounded_traversal_stream(
            256,
            Some((None, Entry::Tree(self.0))),
            move |(path, entry)| {
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
                .boxed()
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
        .await
    }
}

fn files_reference(files: BTreeMap<&str, &str>) -> Result<BTreeMap<MPath, String>> {
    files
        .into_iter()
        .map(|(path, content)| Ok((MPath::new(path)?, content.to_string())))
        .collect()
}

#[test]
fn test_path_tree() -> Result<()> {
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
fn test_path_insert_and_merge() -> Result<()> {
    let mut tree = PathTree::<Vec<_>>::default();
    let items = vec![
        (MPath::new("/one/two/three")?, true),
        (MPath::new("/one/two/three")?, false),
    ];
    for (path, value) in items {
        tree.insert_and_merge(Some(path), value);
    }

    let reference = vec![
        (None, vec![]),
        (Some(MPath::new("one")?), vec![]),
        (Some(MPath::new("one/two")?), vec![]),
        (Some(MPath::new("one/two/three")?), vec![true, false]),
    ];

    assert_eq!(Vec::from_iter(tree), reference);
    Ok(())
}

#[fbinit::test]
async fn test_derive_manifest(fb: FacebookInit) -> Result<()> {
    let blobstore: Arc<dyn Blobstore> = Arc::new(Memblob::default());
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blobstore);

    // derive manifest
    let derive = {
        move |parents, changes| async move {
            derive_test_manifest(ctx, blobstore, parents, changes)
                .await
                .map(|mf| mf.expect("expect non empty manifest"))
        }
    };

    // load all files for specified manifest
    let files = {
        move |manifest_id| async move { Loadable::load(&Files(manifest_id), ctx, blobstore).await }
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
            ctx,
            blobstore,
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

    // Weird case - if a directory is deleted as a file, then
    // it should just be ignored
    {
        let mf0 = derive(
            vec![],
            btreemap! {
                "/one/one" => Some("one"),
            },
        )
        .await?;
        let mf1 = derive(
            vec![mf0],
            btreemap! {
                "/one" => None,
            },
        )
        .await?;
        assert_eq!(
            files_reference(btreemap! {
                "/one/one" => "one",
            })?,
            files(mf0).await?,
        );
        assert_eq!(
            files_reference(btreemap! {
                "/one/one" => "one",
            })?,
            files(mf1).await?,
        );
    }

    // Weird case - deleting non-existing file is fine
    {
        let mf0 = derive(
            vec![],
            btreemap! {
                "/file" => Some("content"),
                "/one/one" => None,
            },
        )
        .await?;
        assert_eq!(
            files_reference(btreemap! {
                "/file" => "content",
            })?,
            files(mf0).await?,
        );
    }

    Ok(())
}

#[fbinit::test]
async fn test_derive_stack_of_manifests(fb: FacebookInit) -> Result<()> {
    let blobstore: Arc<dyn Blobstore> = Arc::new(Memblob::default());
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blobstore);

    // derive manifest
    let derive = {
        move |parents, changes| async move {
            derive_test_stack_of_manifests(ctx, blobstore, parents, changes).await
        }
    };

    // load all files for specified manifest
    let files = {
        move |manifest_id: TestManifestIdU64| async move {
            Loadable::load(&Files(manifest_id), ctx, blobstore).await
        }
    };

    // Simple case - add files one after another
    {
        let mfs = derive(
            None,
            btreemap! {
                ONES_CSID => btreemap! {
                    "/one/one" => Some("one"),
                },
                TWOS_CSID => btreemap! {
                    "/two/two" => Some("two"),
                },
                THREES_CSID => btreemap! {
                    "/three/three" => Some("three"),
                },
            },
        )
        .await?;

        let mfid1 = *mfs.get(&ONES_CSID).unwrap();
        assert_eq!(
            files_reference(btreemap! {
                "/one/one" => "one",
            })?,
            files(mfid1).await?,
        );
        let mfid2 = *mfs.get(&TWOS_CSID).unwrap();
        assert_eq!(
            files_reference(btreemap! {
                "/one/one" => "one",
                "/two/two" => "two",
            })?,
            files(mfid2).await?,
        );
        let mfid3 = *mfs.get(&THREES_CSID).unwrap();
        assert_eq!(
            files_reference(btreemap! {
                "/one/one" => "one",
                "/two/two" => "two",
                "/three/three" => "three",
            })?,
            files(mfid3).await?,
        );
    }

    // Stack which creates/deleted and recreates a few files
    {
        let mfs = derive(
            None,
            btreemap! {
                ONES_CSID => btreemap! {
                    "/one/two" => Some("two"),
                    "/one/three" => Some("three"),
                    "/five/six" => Some("six"),
                },
                TWOS_CSID => btreemap! {
                    "/one/two" => None,
                    "/one/three" => None,
                },
                THREES_CSID => btreemap! {
                    "/one/two" => Some("two_recreated"),
                    "/one/three" => Some("three_recreated"),
                    "/one/four" => Some("four"),
                },
            },
        )
        .await?;

        let mfid1 = *mfs.get(&ONES_CSID).unwrap();
        assert_eq!(
            files_reference(btreemap! {
                "/one/two" => "two",
                "/one/three" => "three",
                "/five/six" => "six",
            })?,
            files(mfid1).await?,
        );
        let mfid2 = *mfs.get(&TWOS_CSID).unwrap();
        assert_eq!(
            files_reference(btreemap! {
                "/five/six" => "six",
            })?,
            files(mfid2).await?,
        );
        let mfid3 = *mfs.get(&THREES_CSID).unwrap();
        assert_eq!(
            files_reference(btreemap! {
                "/one/two" => "two_recreated",
                "/one/three" => "three_recreated",
                "/one/four" => "four",
                "/five/six" => "six",
            })?,
            files(mfid3).await?,
        );
    }

    // Check that parents are processed correctly
    {
        let mfs = derive(
            None,
            btreemap! {
                ONES_CSID => btreemap! {
                    "/one/two" => Some("two"),
                    "/one/three" => Some("three"),
                    "/five/six" => Some("six"),
                },
            },
        )
        .await?;
        let mfid1 = *mfs.get(&ONES_CSID).unwrap();

        let mfs = derive(
            Some(mfid1),
            btreemap! {
                TWOS_CSID => btreemap! {
                    "/one/two" => None,
                    "/one/three" => None,
                },
                THREES_CSID => btreemap! {
                    "/one/two" => Some("two_recreated"),
                    "/one/three" => Some("three_recreated"),
                    "/one/four" => Some("four"),
                },
            },
        )
        .await?;
        let mfid2 = *mfs.get(&TWOS_CSID).unwrap();
        assert_eq!(
            files_reference(btreemap! {
                "/five/six" => "six",
            })?,
            files(mfid2).await?,
        );
        let mfid3 = *mfs.get(&THREES_CSID).unwrap();
        assert_eq!(
            files_reference(btreemap! {
                "/one/two" => "two_recreated",
                "/one/three" => "three_recreated",
                "/one/four" => "four",
                "/five/six" => "six",
            })?,
            files(mfid3).await?,
        );
    }

    // Simple file/dir conflict is resolved
    {
        let mfs = derive(
            None,
            btreemap! {
                ONES_CSID => btreemap! {
                    "/one/two" => Some("two"),
                    "/one/three" => Some("three"),
                },
            },
        )
        .await?;

        let mfid1 = *mfs.get(&ONES_CSID).unwrap();

        let mfs = derive(
            Some(mfid1),
            btreemap! {
                TWOS_CSID => btreemap! {
                    "/one" => Some("resolved_file_dir"),
                },
            },
        )
        .await?;
        let mfid2 = *mfs.get(&TWOS_CSID).unwrap();
        assert_eq!(
            files_reference(btreemap! {
                "/one" => "resolved_file_dir",
            })?,
            files(mfid2).await?,
        );
    }

    // We don't allow paths that are prefixes of each other - raise an error
    {
        let res = derive(
            None,
            btreemap! {
                ONES_CSID => btreemap! {
                    "/one/two" => Some("two"),
                },
                TWOS_CSID => btreemap! {
                    "/one" => Some("overridden"),
                },
            },
        )
        .await;

        assert!(res.is_err());
    }

    // Now let's do an empty commit
    {
        let mfs = derive(
            None,
            btreemap! {
                ONES_CSID => btreemap! {
                    "/one/two" => Some("two"),
                },
                TWOS_CSID => btreemap!{}
            },
        )
        .await?;
        let mfid1 = *mfs.get(&ONES_CSID).unwrap();
        let mfid2 = *mfs.get(&TWOS_CSID).unwrap();
        assert_eq!(mfid1, mfid2);
    }

    // Weird case - if a directory is deleted as a file, then
    // it should just be ignored
    {
        let mfs = derive(
            None,
            btreemap! {
                ONES_CSID => btreemap! {
                    "/one/one" => Some("one"),
                },
            },
        )
        .await?;

        let mfid1 = *mfs.get(&ONES_CSID).unwrap();
        let mfs = derive(
            Some(mfid1),
            btreemap! {
                TWOS_CSID => btreemap! {
                    "/one" => None,
                },
            },
        )
        .await?;

        assert_eq!(
            files_reference(btreemap! {
                "/one/one" => "one",
            })?,
            files(mfid1).await?,
        );
        let mfid2 = *mfs.get(&TWOS_CSID).unwrap();
        assert_eq!(
            files_reference(btreemap! {
                "/one/one" => "one",
            })?,
            files(mfid2).await?,
        );
    }

    // Weird case - deleting non-existing file is fine
    {
        let mfs = derive(
            None,
            btreemap! {
                ONES_CSID => btreemap! {
                    "/file" => Some("content"),
                    "/one/one" => None,
                },
            },
        )
        .await?;

        let mfid1 = *mfs.get(&ONES_CSID).unwrap();
        assert_eq!(
            files_reference(btreemap! {
                "/file" => "content",
            })?,
            files(mfid1).await?,
        );
    }

    // Replace a dir with a file in a single commit
    {
        let mfs = derive(
            None,
            btreemap! {
                ONES_CSID => btreemap! {
                    "/one" => Some("one"),
                },
            },
        )
        .await?;

        let mfid1 = *mfs.get(&ONES_CSID).unwrap();

        let mfs = derive(
            Some(mfid1),
            btreemap! {
                TWOS_CSID => btreemap! {
                    "/one" => None,
                    "/one/one" => Some("one"),
                },
            },
        )
        .await?;
        let mfid2 = *mfs.get(&TWOS_CSID).unwrap();
        assert_eq!(
            files_reference(btreemap! {
                "/one/one" => "one",
            })?,
            files(mfid2).await?,
        );
    }

    // Replace a dir with a file in a single commit
    {
        let mfs = derive(
            None,
            btreemap! {
                ONES_CSID => btreemap! {
                    "/dir/one" => Some("one"),
                },
            },
        )
        .await?;

        let mfid1 = *mfs.get(&ONES_CSID).unwrap();

        let mfs = derive(
            Some(mfid1),
            btreemap! {
                TWOS_CSID => btreemap! {
                    "/dir/anotherfile" => Some("anotherfile"),
                },
                THREES_CSID => btreemap! {
                    "/dir/one" => None,
                    "/dir/one/one" => Some("one"),
                },
            },
        )
        .await?;

        let mfid2 = *mfs.get(&TWOS_CSID).unwrap();
        assert_eq!(
            files_reference(btreemap! {
                "/dir/one" => "one",
                "/dir/anotherfile" => "anotherfile",
            })?,
            files(mfid2).await?,
        );

        let mfid3 = *mfs.get(&THREES_CSID).unwrap();
        assert_eq!(
            files_reference(btreemap! {
                "/dir/one/one" => "one",
                "/dir/anotherfile" => "anotherfile",
            })?,
            files(mfid3).await?,
        );
    }

    Ok(())
}

fn make_paths(paths_str: &[&str]) -> Result<BTreeSet<Option<MPath>>> {
    paths_str
        .iter()
        .map(|path_str| match path_str {
            &"/" => Ok(None),
            _ => MPath::new(path_str).map(Some),
        })
        .collect()
}

fn describe_diff_item(diff: Diff<Entry<TestManifestIdU64, TestLeafId>>) -> String {
    match diff {
        Diff::Added(path, entry) => format!(
            "A {}{}",
            MPath::display_opt(path.as_ref()),
            if entry.is_tree() { "/" } else { "" }
        ),
        Diff::Removed(path, entry) => format!(
            "R {}{}",
            MPath::display_opt(path.as_ref()),
            if entry.is_tree() { "/" } else { "" }
        ),
        Diff::Changed(path, entry, _) => format!(
            "C {}{}",
            MPath::display_opt(path.as_ref()),
            if entry.is_tree() { "/" } else { "" }
        ),
    }
}

#[fbinit::test]
async fn test_find_entries(fb: FacebookInit) -> Result<()> {
    let blobstore: Arc<dyn Blobstore> = Arc::new(Memblob::default());
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blobstore);

    let mf0 = derive_test_manifest(
        ctx,
        blobstore,
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
async fn test_diff(fb: FacebookInit) -> Result<()> {
    let blobstore: Arc<dyn Blobstore> = Arc::new(Memblob::default());
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blobstore);

    let mf0 = derive_test_manifest(
        ctx,
        blobstore,
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
        ctx,
        blobstore,
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
        .diff_ordered(ctx.clone(), blobstore.clone(), mf1, None)
        .map_ok(describe_diff_item)
        .try_collect()
        .await?;

    assert_eq!(
        diffs,
        vec![
            "C (none)/",
            "A added_file",
            "C dir/",
            "A dir/added_dir/",
            "A dir/added_dir/3",
            "C dir/changed_file",
            "R dir/removed_dir/",
            "R dir/removed_dir/3",
            "R dir/removed_file",
            "A dir_file_conflict",
            "R dir_file_conflict/",
            "R dir_file_conflict/5",
        ]
    );

    let diffs: Vec<_> = mf0
        .filtered_diff(
            ctx.clone(),
            blobstore.clone(),
            mf1,
            blobstore.clone(),
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

    let diffs: Vec<_> = mf0
        .filtered_diff_ordered(
            ctx.clone(),
            blobstore.clone(),
            mf1,
            blobstore.clone(),
            Some(Some(MPath::new("dir")?)),
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
        .map_ok(describe_diff_item)
        .try_collect()
        .await?;

    assert_eq!(
        diffs,
        vec![
            "A dir/added_dir/",
            "C dir/changed_file",
            "R dir/removed_file",
            "A dir_file_conflict",
            "R dir_file_conflict/",
            "R dir_file_conflict/5",
        ]
    );
    Ok(())
}

#[fbinit::test]
async fn test_find_intersection_of_diffs(fb: FacebookInit) -> Result<()> {
    let blobstore: Arc<dyn Blobstore> = Arc::new(Memblob::default());
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blobstore);

    let mf0 = derive_test_manifest(
        ctx,
        blobstore,
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
        ctx,
        blobstore,
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
        ctx,
        blobstore,
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
    let intersection: Vec<_> =
        find_intersection_of_diffs(ctx.clone(), blobstore.clone(), mf1, vec![mf0, mf2])
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

#[async_trait]
impl StoreLoadable<ManifestStore> for TestManifestIdStr {
    type Value = TestManifestStr;

    async fn load<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        store: &'a ManifestStore,
    ) -> Result<Self::Value, LoadableError> {
        store
            .0
            .get(self)
            .cloned()
            .ok_or_else(|| LoadableError::Missing(format!("missing {}", self.0)))
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
    MPathElement::new(s.as_bytes().to_vec()).unwrap()
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

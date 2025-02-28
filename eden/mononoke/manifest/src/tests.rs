/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use blobstore::Blobstore;
use borrowed::borrowed;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream::TryStreamExt;
use maplit::btreemap;
use memblob::Memblob;
use mononoke_macros::mononoke;
use mononoke_types::path::MPath;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use mononoke_types_mocks::changesetid::ONES_CSID;
use mononoke_types_mocks::changesetid::THREES_CSID;
use mononoke_types_mocks::changesetid::TWOS_CSID;
use pretty_assertions::assert_eq;

pub(crate) use crate::find_intersection_of_diffs;
pub(crate) use crate::Diff;
pub(crate) use crate::Entry;
pub(crate) use crate::ManifestOps;
pub(crate) use crate::ManifestOrderedOps;
pub(crate) use crate::PathOrPrefix;

pub mod test_manifest;

use self::test_manifest::derive_stack_of_test_manifests;
use self::test_manifest::derive_test_manifest;
use self::test_manifest::list_test_manifest;
use self::test_manifest::TestLeafId;
use self::test_manifest::TestManifestId;

fn files_reference(files: BTreeMap<&str, &str>) -> Result<BTreeMap<NonRootMPath, String>> {
    files
        .into_iter()
        .map(|(path, content)| Ok((NonRootMPath::new(path)?, content.to_string())))
        .collect()
}

#[mononoke::fbinit_test]
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
    let files = move |manifest_id| list_test_manifest(ctx, blobstore, manifest_id);

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

        // make sure conflict is detected
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

        // make sure conflict is detected
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

#[mononoke::fbinit_test]
async fn test_derive_stack_of_manifests(fb: FacebookInit) -> Result<()> {
    let blobstore: Arc<dyn Blobstore> = Arc::new(Memblob::default());
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blobstore);

    // derive manifest
    let derive = {
        move |parents, changes| async move {
            derive_stack_of_test_manifests(ctx, blobstore, parents, changes).await
        }
    };

    // load all files for specified manifest
    let files = move |manifest_id| list_test_manifest(ctx, blobstore, manifest_id);

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

fn make_paths(paths_str: &[&str]) -> Result<Vec<MPath>> {
    let mut paths = paths_str
        .iter()
        .map(|path_str| match path_str {
            &"/" => Ok(MPath::ROOT),
            _ => MPath::new(path_str),
        })
        .collect::<Result<Vec<_>>>()?;
    paths.sort();
    Ok(paths)
}

fn describe_diff_item(diff: Diff<Entry<TestManifestId, (FileType, TestLeafId)>>) -> String {
    match diff {
        Diff::Added(path, entry) => format!(
            "A {}{}",
            MPath::display_opt(&path),
            if entry.is_tree() { "/" } else { "" }
        ),
        Diff::Removed(path, entry) => format!(
            "R {}{}",
            MPath::display_opt(&path),
            if entry.is_tree() { "/" } else { "" }
        ),
        Diff::Changed(path, entry, _) => format!(
            "C {}{}",
            MPath::display_opt(&path),
            if entry.is_tree() { "/" } else { "" }
        ),
    }
}

#[mononoke::fbinit_test]
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

        let mut leafs = Vec::new();
        let mut trees = Vec::new();
        for (path, entry) in results {
            match entry {
                Entry::Tree(_) => trees.push(path),
                Entry::Leaf(_) => leafs.push(path),
            };
        }
        leafs.sort();
        trees.sort();

        assert_eq!(leafs, make_paths(&["one/1", "two/three/four/7",])?);
        assert_eq!(trees, make_paths(&["two/three"])?);
    }

    // use prefix + single select
    {
        let paths = vec![
            PathOrPrefix::Path(MPath::new("two/three/5")?),
            PathOrPrefix::Path(MPath::new("five/seven/11")?),
            PathOrPrefix::Prefix(MPath::new("five/seven/eight")?),
        ];

        let results: Vec<_> = mf0
            .find_entries(ctx.clone(), blobstore.clone(), paths)
            .try_collect()
            .await?;

        let mut leafs = Vec::new();
        let mut trees = Vec::new();
        for (path, entry) in results {
            match entry {
                Entry::Tree(_) => trees.push(path),
                Entry::Leaf(_) => leafs.push(path),
            };
        }
        leafs.sort();
        trees.sort();

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
        let mf_root = mf0
            .find_entry(ctx.clone(), blobstore.clone(), MPath::ROOT)
            .await?;
        assert_eq!(mf_root, Some(Entry::Tree(mf0)));

        let paths = make_paths(&[
            "one/1",
            "two/three",
            "none",
            "two/three/four/7",
            "two/three/6",
            "five/seven/11",
        ])?;

        let mut leafs = Vec::new();
        let mut trees = Vec::new();
        for path in paths {
            let entry = mf0
                .find_entry(ctx.clone(), blobstore.clone(), path.clone())
                .await?;
            match entry {
                Some(Entry::Tree(_)) => {
                    trees.push(path);
                }
                Some(Entry::Leaf(_)) => {
                    leafs.push(path);
                }
                _ => {}
            };
        }
        leafs.sort();
        trees.sort();

        assert_eq!(
            leafs,
            make_paths(&["one/1", "two/three/four/7", "five/seven/11"])?
        );
        assert_eq!(trees, make_paths(&["two/three"])?);
    }

    Ok(())
}

#[mononoke::fbinit_test]
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

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for diff in diffs {
        match diff {
            Diff::Added(path, _) => {
                added.push(path);
            }
            Diff::Removed(path, _) => {
                removed.push(path);
            }
            Diff::Changed(path, _, _) => {
                changed.push(path);
            }
        };
    }
    added.sort();
    removed.sort();
    changed.sort();

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
                if &badpath != path { Some(diff) } else { None }
            },
            |diff| {
                let path = match diff {
                    Diff::Added(path, ..) | Diff::Removed(path, ..) | Diff::Changed(path, ..) => {
                        path
                    }
                };

                let badpath = MPath::new("dir/removed_dir").unwrap();
                &badpath != path
            },
        )
        .try_collect()
        .await?;

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for diff in diffs {
        match diff {
            Diff::Added(path, _) => {
                added.push(path);
            }
            Diff::Removed(path, _) => {
                removed.push(path);
            }
            Diff::Changed(path, _, _) => {
                changed.push(path);
            }
        };
    }
    added.sort();
    removed.sort();
    changed.sort();

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
            Some(MPath::new("dir")?),
            |diff| {
                let path = match &diff {
                    Diff::Added(path, ..) | Diff::Removed(path, ..) | Diff::Changed(path, ..) => {
                        path
                    }
                };

                let badpath = MPath::new("dir/added_dir/3").unwrap();
                if &badpath != path { Some(diff) } else { None }
            },
            |diff| {
                let path = match diff {
                    Diff::Added(path, ..) | Diff::Removed(path, ..) | Diff::Changed(path, ..) => {
                        path
                    }
                };

                let badpath = MPath::new("dir/removed_dir").unwrap();
                &badpath != path
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

#[mononoke::fbinit_test]
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

    let mut intersection: Vec<_> =
        find_intersection_of_diffs(ctx.clone(), blobstore.clone(), mf1, vec![mf0])
            .map_ok(|(path, _)| path)
            .try_collect()
            .await?;
    intersection.sort();

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
    let mut intersection: Vec<_> =
        find_intersection_of_diffs(ctx.clone(), blobstore.clone(), mf1, vec![mf0, mf2])
            .map_ok(|(path, _)| path)
            .try_collect()
            .await?;
    intersection.sort();

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

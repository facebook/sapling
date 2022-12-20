/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use blobstore::Loadable;
use bytes::Bytes;
use cacheblob::InProcessLease;
use chrono::FixedOffset;
use chrono::TimeZone;
use cross_repo_sync::update_mapping_with_version;
use cross_repo_sync::CommitSyncRepos;
use cross_repo_sync::CommitSyncer;
use cross_repo_sync_test_utils::init_small_large_repo;
use fbinit::FacebookInit;
use fixtures::BranchUneven;
use fixtures::Linear;
use fixtures::ManyFilesDirs;
use fixtures::TestRepoFixture;
use futures::stream::TryStreamExt;
use futures::FutureExt;
use live_commit_sync_config::TestLiveCommitSyncConfigSource;
use maplit::hashmap;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::DefaultSmallToLargeCommitSyncPathAction;
use mononoke_types::hash::GitSha1;
use mononoke_types::hash::RichGitSha1;
use mononoke_types::hash::Sha1;
use mononoke_types::hash::Sha256;
use mononoke_types::MPath;
use slog::info;
use synced_commit_mapping::ArcSyncedCommitMapping;
use tests_utils::bookmark;
use tests_utils::resolve_cs_id;
use tests_utils::CreateCommitContext;
use tunables::tunables;
use tunables::with_tunables_async;
use tunables::MononokeTunables;

use crate::BookmarkFreshness;
use crate::ChangesetFileOrdering;
use crate::ChangesetId;
use crate::ChangesetIdPrefix;
use crate::ChangesetPrefixSpecifier;
use crate::ChangesetSpecifier;
use crate::ChangesetSpecifierPrefixResolution;
use crate::CoreContext;
use crate::FileId;
use crate::FileMetadata;
use crate::FileType;
use crate::HgChangesetId;
use crate::HgChangesetIdPrefix;
use crate::Mononoke;
use crate::MononokePath;
use crate::TreeEntry;
use crate::TreeId;

#[fbinit::test]
async fn commit_info_by_hash(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), Linear::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx, "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let hash = "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6";
    let cs_id = ChangesetId::from_str(hash)?;
    let cs = repo.changeset(cs_id).await?.expect("changeset exists");

    assert_eq!(cs.message().await?, "modified 10");
    assert_eq!(cs.author().await?, "Jeremy Fitzhardinge <jsgf@fb.com>");
    assert_eq!(
        cs.author_date().await?,
        FixedOffset::west(7 * 3600).timestamp(1504041761, 0)
    );
    assert_eq!(cs.generation().await?.value(), 11);

    Ok(())
}

#[fbinit::test]
async fn commit_info_by_hg_hash(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), Linear::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx, "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let hg_hash = "607314ef579bd2407752361ba1b0c1729d08b281";
    let hg_cs_id = HgChangesetId::from_str(hg_hash)?;
    let cs = repo.changeset(hg_cs_id).await?.expect("changeset exists");

    let hash = "2cb6d2d3052bfbdd6a95a61f2816d81130033b5f5a99e8d8fc24d9238d85bb48";
    assert_eq!(cs.id(), ChangesetId::from_str(hash)?);
    assert_eq!(cs.hg_id().await?, Some(HgChangesetId::from_str(hg_hash)?));
    assert_eq!(cs.message().await?, "added 3");
    assert_eq!(cs.author().await?, "Jeremy Fitzhardinge <jsgf@fb.com>");
    assert_eq!(
        cs.author_date().await?,
        FixedOffset::west(7 * 3600).timestamp(1504041758, 0)
    );

    Ok(())
}

#[fbinit::test]
async fn commit_info_by_bookmark(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), Linear::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx, "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let cs = repo
        .resolve_bookmark("master", BookmarkFreshness::MostRecent)
        .await?
        .expect("bookmark exists");

    let hash = "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6";
    assert_eq!(cs.id(), ChangesetId::from_str(hash)?);
    let hg_hash = "79a13814c5ce7330173ec04d279bf95ab3f652fb";
    assert_eq!(cs.hg_id().await?, Some(HgChangesetId::from_str(hg_hash)?));
    assert_eq!(cs.message().await?, "modified 10");
    assert_eq!(cs.author().await?, "Jeremy Fitzhardinge <jsgf@fb.com>");
    assert_eq!(
        cs.author_date().await?,
        FixedOffset::west(7 * 3600).timestamp(1504041761, 0)
    );

    Ok(())
}

#[fbinit::test]
async fn commit_hg_changeset_ids(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), Linear::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx, "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let hash1 = "2cb6d2d3052bfbdd6a95a61f2816d81130033b5f5a99e8d8fc24d9238d85bb48";
    let hash2 = "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6";
    let hg_hash1 = "607314ef579bd2407752361ba1b0c1729d08b281";
    let hg_hash2 = "79a13814c5ce7330173ec04d279bf95ab3f652fb";
    let ids: HashMap<_, _> = repo
        .many_changeset_hg_ids(vec![
            ChangesetId::from_str(hash1)?,
            ChangesetId::from_str(hash2)?,
        ])
        .await?
        .into_iter()
        .collect();
    assert_eq!(
        ids.get(&ChangesetId::from_str(hash1)?),
        Some(&HgChangesetId::from_str(hg_hash1)?)
    );
    assert_eq!(
        ids.get(&ChangesetId::from_str(hash2)?),
        Some(&HgChangesetId::from_str(hg_hash2)?)
    );

    Ok(())
}

#[fbinit::test]
async fn commit_is_ancestor_of(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), BranchUneven::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx, "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let mut changesets = Vec::new();
    for hg_hash in [
        "5d43888a3c972fe68c224f93d41b30e9f888df7c", // 0: branch 1 near top
        "d7542c9db7f4c77dab4b315edd328edf1514952f", // 1: branch 1 near bottom
        "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5", // 2: branch 2
        "15c40d0abc36d47fb51c8eaec51ac7aad31f669c", // 3: base
    ] {
        let changeset = repo
            .changeset(HgChangesetId::from_str(hg_hash)?)
            .await
            .expect("changeset exists");
        changesets.push(changeset);
    }
    for (index, base_index, is_ancestor_of) in [
        (0usize, 0usize, true),
        (0, 1, false),
        (0, 2, false),
        (0, 3, false),
        (1, 0, true),
        (1, 1, true),
        (1, 2, false),
        (1, 3, false),
        (2, 0, false),
        (2, 1, false),
        (2, 2, true),
        (2, 3, false),
        (3, 0, true),
        (3, 1, true),
        (3, 2, true),
        (3, 3, true),
    ] {
        assert_eq!(
            changesets[index]
                .as_ref()
                .unwrap()
                .is_ancestor_of(changesets[base_index].as_ref().unwrap().id())
                .await?,
            is_ancestor_of,
            "changesets[{}].is_ancestor_of(changesets[{}].id()) == {}",
            index,
            base_index,
            is_ancestor_of
        );
    }
    Ok(())
}

async fn commit_find_files_impl(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), ManyFilesDirs::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx, "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let hash = "b0d1bf77898839595ee0f0cba673dd6e3be9dadaaa78bc6dd2dea97ca6bee77e";
    let cs_id = ChangesetId::from_str(hash)?;
    let cs = repo.changeset(cs_id).await?.expect("changeset exists");

    // Find everything
    let mut files: Vec<_> = cs
        .find_files_unordered(None, None)
        .await?
        .try_collect()
        .await?;
    files.sort();
    let expected_files = vec![
        MononokePath::try_from("1")?,
        MononokePath::try_from("2")?,
        MononokePath::try_from("dir1/file_1_in_dir1")?,
        MononokePath::try_from("dir1/file_2_in_dir1")?,
        MononokePath::try_from("dir1/subdir1/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir1/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_2")?,
        MononokePath::try_from("dir2/file_1_in_dir2")?,
    ];
    assert_eq!(files, expected_files);

    // Find everything ordered
    let files: Vec<_> = cs
        .find_files(
            None,
            None,
            None,
            ChangesetFileOrdering::Ordered { after: None },
        )
        .await?
        .try_collect()
        .await?;
    assert_eq!(files, expected_files);

    // Find everything after a particular file
    let files: Vec<_> = cs
        .find_files(
            None,
            None,
            None,
            ChangesetFileOrdering::Ordered {
                after: Some(MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_2")?,
        MononokePath::try_from("dir2/file_1_in_dir2")?,
    ];
    assert_eq!(files, expected_files);

    // Prefixes
    let mut files: Vec<_> = cs
        .find_files_unordered(
            Some(vec![
                MononokePath::try_from("dir1/subdir1/subsubdir1")?,
                MononokePath::try_from("dir2")?,
            ]),
            None,
        )
        .await?
        .try_collect()
        .await?;
    files.sort();
    let expected_files = vec![
        MononokePath::try_from("dir1/subdir1/subsubdir1/file_1")?,
        MononokePath::try_from("dir2/file_1_in_dir2")?,
    ];
    assert_eq!(files, expected_files);

    // Prefixes ordered, starting at the root.  (This has no real effect on
    // the output vs `after: None`, but exercise the code path anyway).
    let files: Vec<_> = cs
        .find_files(
            Some(vec![
                MononokePath::try_from("dir1/subdir1/subsubdir1")?,
                MononokePath::try_from("dir2")?,
            ]),
            None,
            None,
            ChangesetFileOrdering::Ordered {
                after: Some(MononokePath::try_from("")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    assert_eq!(files, expected_files);

    // Prefixes ordered after
    let files: Vec<_> = cs
        .find_files(
            Some(vec![
                MononokePath::try_from("dir1/subdir1/subsubdir1")?,
                MononokePath::try_from("dir2")?,
            ]),
            None,
            None,
            ChangesetFileOrdering::Ordered {
                after: Some(MononokePath::try_from("dir1/subdir1/subsubdir1/file_1")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![MononokePath::try_from("dir2/file_1_in_dir2")?];
    assert_eq!(files, expected_files);

    // Basenames
    let mut files: Vec<_> = cs
        .find_files_unordered(None, Some(vec![String::from("file_1")]))
        .await?
        .try_collect()
        .await?;
    files.sort();
    let expected_files = vec![
        MononokePath::try_from("dir1/subdir1/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir1/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?,
    ];
    assert_eq!(files, expected_files);

    // Basenames ordered
    let files: Vec<_> = cs
        .find_files(
            None,
            Some(vec![String::from("file_1")]),
            None,
            ChangesetFileOrdering::Ordered {
                after: Some(MononokePath::try_from("")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    assert_eq!(files, expected_files);

    // Basenames ordered after
    let files: Vec<_> = cs
        .find_files(
            None,
            Some(vec![String::from("file_1")]),
            None,
            ChangesetFileOrdering::Ordered {
                after: Some(MononokePath::try_from("dir1/subdir1/subsubdir1/file_1")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?];
    assert_eq!(files, expected_files);

    // Basenames and Prefixes
    let mut files: Vec<_> = cs
        .find_files_unordered(
            Some(vec![
                MononokePath::try_from("dir1/subdir1/subsubdir2")?,
                MononokePath::try_from("dir2")?,
            ]),
            Some(vec![String::from("file_2"), String::from("file_1_in_dir2")]),
        )
        .await?
        .try_collect()
        .await?;
    files.sort();
    let expected_files = vec![
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_2")?,
        MononokePath::try_from("dir2/file_1_in_dir2")?,
    ];
    assert_eq!(files, expected_files);

    // Basenames and Prefixes ordered
    let mut files: Vec<_> = cs
        .find_files(
            Some(vec![
                MononokePath::try_from("dir1/subdir1/subsubdir2")?,
                MononokePath::try_from("dir2")?,
            ]),
            Some(vec![String::from("file_2"), String::from("file_1_in_dir2")]),
            None,
            ChangesetFileOrdering::Ordered {
                after: Some(MononokePath::try_from("")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_2")?,
        MononokePath::try_from("dir2/file_1_in_dir2")?,
    ];
    files.sort();
    assert_eq!(files, expected_files);

    // Basenames and Prefixes ordered after
    let files: Vec<_> = cs
        .find_files(
            Some(vec![
                MononokePath::try_from("dir1/subdir1/subsubdir2")?,
                MononokePath::try_from("dir2")?,
            ]),
            Some(vec![String::from("file_2"), String::from("file_1_in_dir2")]),
            None,
            ChangesetFileOrdering::Ordered {
                after: Some(MononokePath::try_from("dir1a/file_1_in_dir2")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![MononokePath::try_from("dir2/file_1_in_dir2")?];
    assert_eq!(files, expected_files);

    // Suffixes
    let mut files: Vec<_> = cs
        .find_files(
            None,
            None,
            Some(vec![String::from("_1"), String::from("_2")]),
            ChangesetFileOrdering::Unordered,
        )
        .await?
        .try_collect()
        .await?;
    files.sort();
    let expected_files = vec![
        MononokePath::try_from("dir1/subdir1/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir1/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, ordered
    let files: Vec<_> = cs
        .find_files(
            None,
            None,
            Some(vec![String::from("_1"), String::from("_2")]),
            ChangesetFileOrdering::Ordered {
                after: Some(MononokePath::try_from("")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![
        MononokePath::try_from("dir1/subdir1/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir1/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, ordered after
    let files: Vec<_> = cs
        .find_files(
            None,
            None,
            Some(vec![String::from("_1"), String::from("_2")]),
            ChangesetFileOrdering::Ordered {
                after: Some(MononokePath::try_from("dir1/subdir1/subsubdir1/file_1")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, prefixes
    let mut files: Vec<_> = cs
        .find_files(
            Some(vec![
                MononokePath::try_from("dir1/subdir1/subsubdir1")?,
                MononokePath::try_from("dir1/subdir1/subsubdir2")?,
            ]),
            None,
            Some(vec![String::from("1"), String::from("2")]),
            ChangesetFileOrdering::Unordered,
        )
        .await?
        .try_collect()
        .await?;
    files.sort();
    let expected_files = vec![
        MononokePath::try_from("dir1/subdir1/subsubdir1/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, prefixes, ordered
    let files: Vec<_> = cs
        .find_files(
            Some(vec![
                MononokePath::try_from("dir1/subdir1/subsubdir1")?,
                MononokePath::try_from("dir1/subdir1/subsubdir2")?,
            ]),
            None,
            Some(vec![String::from("1"), String::from("2")]),
            ChangesetFileOrdering::Ordered {
                after: Some(MononokePath::try_from("")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![
        MononokePath::try_from("dir1/subdir1/subsubdir1/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, prefixes, ordered after
    let files: Vec<_> = cs
        .find_files(
            Some(vec![
                MononokePath::try_from("dir1/subdir1/subsubdir1")?,
                MononokePath::try_from("dir1/subdir1/subsubdir2")?,
            ]),
            None,
            Some(vec![String::from("1"), String::from("2")]),
            ChangesetFileOrdering::Ordered {
                after: Some(MononokePath::try_from("dir1/subdir1/subsubdir1/file_1")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, basenames
    let mut files: Vec<_> = cs
        .find_files(
            None,
            Some(vec![String::from("file_1_in_dir2")]),
            Some(vec![String::from("1")]),
            ChangesetFileOrdering::Unordered,
        )
        .await?
        .try_collect()
        .await?;
    files.sort();
    let expected_files = vec![
        MononokePath::try_from("1")?,
        MononokePath::try_from("dir1/file_1_in_dir1")?,
        MononokePath::try_from("dir1/file_2_in_dir1")?,
        MononokePath::try_from("dir1/subdir1/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir1/file_1")?,
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MononokePath::try_from("dir2/file_1_in_dir2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, basenames, ordered
    let files: Vec<_> = cs
        .find_files(
            None,
            Some(vec![String::from("file_1_in_dir2")]),
            Some(vec![String::from("1")]),
            ChangesetFileOrdering::Ordered {
                after: Some(MononokePath::try_from("")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    // BSSM have different but consistent orders
    let expected_files = if tunables().get_disable_basename_suffix_skeleton_manifest() {
        vec![
            MononokePath::try_from("1")?,
            MononokePath::try_from("dir1/file_1_in_dir1")?,
            MononokePath::try_from("dir1/file_2_in_dir1")?,
            MononokePath::try_from("dir1/subdir1/file_1")?,
            MononokePath::try_from("dir1/subdir1/subsubdir1/file_1")?,
            MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?,
            MononokePath::try_from("dir2/file_1_in_dir2")?,
        ]
    } else {
        vec![
            MononokePath::try_from("1")?,
            MononokePath::try_from("dir1/subdir1/file_1")?,
            MononokePath::try_from("dir1/subdir1/subsubdir1/file_1")?,
            MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?,
            MononokePath::try_from("dir1/file_1_in_dir1")?,
            MononokePath::try_from("dir1/file_2_in_dir1")?,
            MononokePath::try_from("dir2/file_1_in_dir2")?,
        ]
    };
    assert_eq!(files, expected_files);

    // Suffixes, basenames, ordered after
    let files: Vec<_> = cs
        .find_files(
            None,
            Some(vec![String::from("file_1_in_dir2")]),
            Some(vec![String::from("1")]),
            ChangesetFileOrdering::Ordered {
                after: Some(MononokePath::try_from("dir1/subdir1/subsubdir1/file_1")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = if tunables().get_disable_basename_suffix_skeleton_manifest() {
        vec![
            MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?,
            MononokePath::try_from("dir2/file_1_in_dir2")?,
        ]
    } else {
        vec![
            MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?,
            MononokePath::try_from("dir1/file_1_in_dir1")?,
            MononokePath::try_from("dir1/file_2_in_dir1")?,
            MononokePath::try_from("dir2/file_1_in_dir2")?,
        ]
    };
    assert_eq!(files, expected_files);

    // Suffixes, basenames, prefixes
    let mut files: Vec<_> = cs
        .find_files(
            Some(vec![
                MononokePath::try_from("dir1/subdir1/subsubdir2")?,
                MononokePath::try_from("dir2")?,
            ]),
            Some(vec![String::from("file_1_in_dir2")]),
            Some(vec![String::from("1")]),
            ChangesetFileOrdering::Unordered,
        )
        .await?
        .try_collect()
        .await?;
    files.sort();
    let expected_files = vec![
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MononokePath::try_from("dir2/file_1_in_dir2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, basenames, prefixes, ordered
    let files: Vec<_> = cs
        .find_files(
            Some(vec![
                MononokePath::try_from("dir1/subdir1/subsubdir2")?,
                MononokePath::try_from("dir2")?,
            ]),
            Some(vec![String::from("file_1_in_dir2")]),
            Some(vec![String::from("1")]),
            ChangesetFileOrdering::Ordered {
                after: Some(MononokePath::try_from("")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![
        MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?,
        MononokePath::try_from("dir2/file_1_in_dir2")?,
    ];
    assert_eq!(files, expected_files);

    // Suffixes, basenames, prefixes, ordered after
    let files: Vec<_> = cs
        .find_files(
            Some(vec![
                MononokePath::try_from("dir1/subdir1/subsubdir2")?,
                MononokePath::try_from("dir2")?,
            ]),
            Some(vec![String::from("file_1_in_dir2")]),
            Some(vec![String::from("1")]),
            ChangesetFileOrdering::Ordered {
                after: Some(MononokePath::try_from("dir1/subdir1/subsubdir2/file_1")?),
            },
        )
        .await?
        .try_collect()
        .await?;
    let expected_files = vec![MononokePath::try_from("dir2/file_1_in_dir2")?];
    assert_eq!(files, expected_files);

    Ok(())
}

#[fbinit::test]
async fn commit_find_files(fb: FacebookInit) {
    commit_find_files_impl(fb).await.unwrap();
}

#[fbinit::test]
async fn commit_find_files_without_bssm(fb: FacebookInit) {
    let tunables = MononokeTunables::default();
    tunables.update_bools(&hashmap! {
        "disable_basename_suffix_skeleton_manifest".to_string() => true
    });
    with_tunables_async(tunables, commit_find_files_impl(fb).boxed())
        .await
        .unwrap();
}

#[fbinit::test]
async fn commit_path_exists_and_type(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), ManyFilesDirs::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx, "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let hash = "b0d1bf77898839595ee0f0cba673dd6e3be9dadaaa78bc6dd2dea97ca6bee77e";
    let cs_id = ChangesetId::from_str(hash)?;
    let cs = repo.changeset(cs_id).await?.expect("changeset exists");

    let root_path = cs.root().await?;
    assert!(root_path.exists().await?);
    assert!(root_path.is_tree().await?);

    let dir1_path = cs.path_with_content("dir1").await?;
    assert!(dir1_path.exists().await?);
    assert!(dir1_path.is_tree().await?);
    assert_eq!(dir1_path.file_type().await?, None);

    let file1_path = cs.path_with_content("dir1/file_1_in_dir1").await?;
    assert!(file1_path.exists().await?);
    assert!(!file1_path.is_tree().await?);
    assert_eq!(file1_path.file_type().await?, Some(FileType::Regular));

    let nonexistent_path = cs.path_with_content("nonexistent").await?;
    assert!(!nonexistent_path.exists().await?);
    assert!(!nonexistent_path.is_tree().await?);
    assert_eq!(nonexistent_path.file_type().await?, None);

    Ok(())
}

#[fbinit::test]
async fn tree_list(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), ManyFilesDirs::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx, "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let hash = "b0d1bf77898839595ee0f0cba673dd6e3be9dadaaa78bc6dd2dea97ca6bee77e";
    let cs_id = ChangesetId::from_str(hash)?;
    let cs = repo.changeset(cs_id).await?.expect("changeset exists");
    assert_eq!(
        {
            let path = cs.root().await?;
            let tree = path.tree().await?.unwrap();
            tree.list()
                .await?
                .into_iter()
                .map(|(name, _entry)| name)
                .collect::<Vec<_>>()
        },
        vec![
            String::from("1"),
            String::from("2"),
            String::from("dir1"),
            String::from("dir2")
        ]
    );
    assert_eq!(
        {
            let path = cs.path_with_content("dir1").await?;
            let tree = path.tree().await?.unwrap();
            tree.list()
                .await?
                .into_iter()
                .map(|(name, _entry)| name)
                .collect::<Vec<_>>()
        },
        vec![
            String::from("file_1_in_dir1"),
            String::from("file_2_in_dir1"),
            String::from("subdir1"),
        ]
    );
    let subsubdir2_id = {
        // List `dir1/subdir1`, but also capture a subtree id.
        let path = cs.path_with_content("dir1/subdir1").await?;
        let tree = path.tree().await?.unwrap();
        assert_eq!(
            {
                tree.list()
                    .await?
                    .into_iter()
                    .map(|(name, _entry)| name)
                    .collect::<Vec<_>>()
            },
            vec![
                String::from("file_1"),
                String::from("subsubdir1"),
                String::from("subsubdir2")
            ]
        );
        match tree
            .list()
            .await?
            .into_iter()
            .collect::<HashMap<_, _>>()
            .get("subsubdir2")
            .expect("entry should exist for subsubdir2")
        {
            TreeEntry::Directory(dir) => dir.id().clone(),
            entry => panic!("subsubdir2 entry should be a directory, not {:?}", entry),
        }
    };
    assert_eq!(
        {
            let path = cs.path_with_content("dir1/subdir1/subsubdir1").await?;
            let tree = path.tree().await?.unwrap();
            tree.list()
                .await?
                .into_iter()
                .map(|(name, entry)| match entry {
                    TreeEntry::File(file) => {
                        Some((name, file.size(), file.content_sha1().to_string()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        },
        vec![Some((
            String::from("file_1"),
            9,
            String::from("aa02177d2c1f3af3fb5b7b25698cb37772b1226b")
        ))]
    );
    // Get tree by id
    assert_eq!(
        {
            let tree = repo.tree(subsubdir2_id).await?.expect("tree exists");
            tree.list()
                .await?
                .into_iter()
                .map(|(name, _entry)| name)
                .collect::<Vec<_>>()
        },
        vec![String::from("file_1"), String::from("file_2")]
    );
    // Get tree by non-existent id returns None.
    assert!(
        repo.tree(TreeId::from_bytes([1; 32]).unwrap())
            .await?
            .is_none()
    );
    // Get tree by non-existent path returns None.
    {
        let path = cs.path_with_content("nonexistent").await?;
        assert!(path.tree().await?.is_none());
    }

    Ok(())
}

#[fbinit::test]
async fn file_metadata(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), ManyFilesDirs::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx, "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let expected_metadata = FileMetadata {
        content_id: FileId::from_str(
            "9d9cf646b38852094ec48ab401eea6f4481cc89a80589331845dc08f75a652d2",
        )?,
        total_size: 9,
        sha1: Sha1::from_str("b29930dda02406077d96a7b7a08ce282b3de6961")?,
        sha256: Sha256::from_str(
            "47d741b6059c6d7e99be25ce46fb9ba099cfd6515de1ef7681f93479d25996a4",
        )?,
        git_sha1: RichGitSha1::from_sha1(
            GitSha1::from_str("ac3e272b72bbf89def8657766b855d0656630ed4")?,
            "blob",
            9,
        ),
    };

    // Get file by changeset path.
    let hash = "b0d1bf77898839595ee0f0cba673dd6e3be9dadaaa78bc6dd2dea97ca6bee77e";
    let cs_id = ChangesetId::from_str(hash)?;
    let cs = repo.changeset(cs_id).await?.expect("changeset exists");

    let path = cs.path_with_content("dir1/file_1_in_dir1").await?;
    let file = path.file().await?.unwrap();
    let metadata = file.metadata().await?;
    assert_eq!(metadata, expected_metadata);

    // Get file by content id.
    let file = repo
        .file(FileId::from_str(
            "9d9cf646b38852094ec48ab401eea6f4481cc89a80589331845dc08f75a652d2",
        )?)
        .await?
        .expect("file exists");
    let metadata = file.metadata().await?;
    assert_eq!(metadata, expected_metadata);

    // Get file by content sha1.
    let file = repo
        .file_by_content_sha1(Sha1::from_str("b29930dda02406077d96a7b7a08ce282b3de6961")?)
        .await?
        .expect("file exists");
    let metadata = file.metadata().await?;
    assert_eq!(metadata, expected_metadata);

    // Get file by content sha256.
    let file = repo
        .file_by_content_sha256(Sha256::from_str(
            "47d741b6059c6d7e99be25ce46fb9ba099cfd6515de1ef7681f93479d25996a4",
        )?)
        .await?
        .expect("file exists");
    let metadata = file.metadata().await?;
    assert_eq!(metadata, expected_metadata);

    Ok(())
}

#[fbinit::test]
async fn file_contents(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), ManyFilesDirs::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx, "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let hash = "b0d1bf77898839595ee0f0cba673dd6e3be9dadaaa78bc6dd2dea97ca6bee77e";
    let cs_id = ChangesetId::from_str(hash)?;
    let cs = repo.changeset(cs_id).await?.expect("changeset exists");

    let path = cs.path_with_content("dir1/file_1_in_dir1").await?;
    let file = path.file().await?.unwrap();
    let content = file.content_concat().await?;
    assert_eq!(content, Bytes::from("content1\n"));

    let content_range = file.content_range_concat(3, 4).await?;
    assert_eq!(content_range, Bytes::from("tent"));

    Ok(())
}

#[fbinit::test]
async fn xrepo_commit_lookup_simple(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (mononoke, _cfg_src) = init_x_repo(&ctx).await?;

    let smallrepo = mononoke
        .repo(ctx.clone(), "smallrepo")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let largerepo = mononoke
        .repo(ctx.clone(), "largerepo")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let small_master_cs_id = resolve_cs_id(&ctx, smallrepo.blob_repo(), "master").await?;

    info!(
        ctx.logger(),
        "remapping {} from small to large", small_master_cs_id
    );
    // Confirm that a cross-repo lookup for an unsynced commit just fails
    let cs = smallrepo
        .xrepo_commit_lookup(&largerepo, small_master_cs_id, None)
        .await?
        .expect("changeset should exist");
    let large_master_cs_id = resolve_cs_id(&ctx, largerepo.blob_repo(), "master").await?;
    assert_eq!(cs.id(), large_master_cs_id);

    info!(
        ctx.logger(),
        "remapping {} from large to small", large_master_cs_id
    );
    let cs = largerepo
        .xrepo_commit_lookup(&smallrepo, large_master_cs_id, None)
        .await?
        .expect("changeset should exist");
    assert_eq!(cs.id(), small_master_cs_id);
    Ok(())
}

#[fbinit::test]
async fn xrepo_commit_lookup_draft(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (mononoke, _cfg_src) = init_x_repo(&ctx).await?;

    let smallrepo = mononoke
        .repo(ctx.clone(), "smallrepo")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let small_master_cs_id = resolve_cs_id(&ctx, smallrepo.blob_repo(), "master").await?;
    let largerepo = mononoke
        .repo(ctx.clone(), "largerepo")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let large_master_cs_id = resolve_cs_id(&ctx, largerepo.blob_repo(), "master").await?;

    let new_large_draft =
        CreateCommitContext::new(&ctx, largerepo.blob_repo(), vec![large_master_cs_id])
            .add_file("prefix/remapped", "content1")
            .add_file("not_remapped", "content2")
            .commit()
            .await?;

    let cs = largerepo
        .xrepo_commit_lookup(&smallrepo, new_large_draft, None)
        .await?;
    assert!(cs.is_some());
    let bcs = cs
        .unwrap()
        .id()
        .load(&ctx, smallrepo.blob_repo().blobstore())
        .await?;
    let file_changes: Vec<_> = bcs.file_changes().map(|(path, _)| path).cloned().collect();
    assert_eq!(file_changes, vec![MPath::new("remapped")?]);

    // Now in another direction
    let new_small_draft =
        CreateCommitContext::new(&ctx, smallrepo.blob_repo(), vec![small_master_cs_id])
            .add_file("remapped2", "content2")
            .commit()
            .await?;
    let cs = smallrepo
        .xrepo_commit_lookup(&largerepo, new_small_draft, None)
        .await?;
    assert!(cs.is_some());
    let bcs = cs
        .unwrap()
        .id()
        .load(&ctx, largerepo.blob_repo().blobstore())
        .await?;
    let file_changes: Vec<_> = bcs.file_changes().map(|(path, _)| path).cloned().collect();
    assert_eq!(file_changes, vec![MPath::new("prefix/remapped2")?]);

    Ok(())
}

#[fbinit::test]
async fn xrepo_commit_lookup_public(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (mononoke, _cfg_src) = init_x_repo(&ctx).await?;

    let smallrepo = mononoke
        .repo(ctx.clone(), "smallrepo")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let small_master_cs_id = resolve_cs_id(&ctx, smallrepo.blob_repo(), "master").await?;
    let largerepo = mononoke
        .repo(ctx.clone(), "largerepo")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let large_master_cs_id = resolve_cs_id(&ctx, largerepo.blob_repo(), "master").await?;

    let new_large_public =
        CreateCommitContext::new(&ctx, largerepo.blob_repo(), vec![large_master_cs_id])
            .add_file("prefix/remapped", "content1")
            .add_file("not_remapped", "content2")
            .commit()
            .await?;

    bookmark(&ctx, largerepo.blob_repo(), "publicbook")
        .set_to(new_large_public)
        .await?;

    let cs = largerepo
        .xrepo_commit_lookup(&smallrepo, new_large_public, None)
        .await?;
    assert!(cs.is_some());
    let bcs = cs
        .unwrap()
        .id()
        .load(&ctx, smallrepo.blob_repo().blobstore())
        .await?;
    let file_changes: Vec<_> = bcs.file_changes().map(|(path, _)| path).cloned().collect();
    assert_eq!(file_changes, vec![MPath::new("remapped")?]);

    // Now in another direction - it should fail
    let new_small_public =
        CreateCommitContext::new(&ctx, smallrepo.blob_repo(), vec![small_master_cs_id])
            .add_file("remapped2", "content2")
            .commit()
            .await?;
    bookmark(&ctx, smallrepo.blob_repo(), "newsmallpublicbook")
        .set_to(new_small_public)
        .await?;
    let res = smallrepo
        .xrepo_commit_lookup(&largerepo, new_small_public, None)
        .await;
    assert!(res.is_err());

    Ok(())
}

#[fbinit::test]
async fn xrepo_commit_lookup_config_changing_live(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (mononoke, cfg_src) = init_x_repo(&ctx).await?;

    let smallrepo = mononoke
        .repo(ctx.clone(), "smallrepo")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let largerepo = mononoke
        .repo(ctx.clone(), "largerepo")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let small_master_cs_id = resolve_cs_id(&ctx, smallrepo.blob_repo(), "master").await?;
    let large_master_cs_id = resolve_cs_id(&ctx, largerepo.blob_repo(), "master").await?;

    // Before config change
    let first_large =
        CreateCommitContext::new(&ctx, largerepo.blob_repo(), vec![large_master_cs_id])
            .add_file("prefix/remapped_before", "content1")
            .add_file("not_remapped", "content2")
            .commit()
            .await?;

    let first_small = largerepo
        .xrepo_commit_lookup(&smallrepo, first_large, None)
        .await?;
    let file_changes: Vec<_> = first_small
        .unwrap()
        .id()
        .load(&ctx, smallrepo.blob_repo().blobstore())
        .await?
        .file_changes()
        .map(|(path, _)| path)
        .cloned()
        .collect();

    assert_eq!(file_changes, vec![MPath::new("remapped_before")?]);

    // Config change: new config remaps prefix2 instead of prefix
    let large_repo_id = largerepo.blob_repo().get_repoid();
    let small_repo_id = smallrepo.blob_repo().get_repoid();
    let mut cfg = cfg_src
        .get_commit_sync_config_by_version_if_exists(
            large_repo_id,
            &CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
        )?
        .unwrap();
    let common_config = cfg_src
        .get_common_config_if_exists(large_repo_id)?
        .ok_or_else(|| anyhow!("common config doesn't exist"))?;
    cfg.small_repos
        .get_mut(&small_repo_id)
        .unwrap()
        .default_action =
        DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(MPath::new("prefix2").unwrap());
    let new_version = CommitSyncConfigVersion("TEST_VERSION_NAME_2".to_string());
    cfg.version_name = new_version.clone();
    cfg_src.add_config(cfg.clone());

    let change_mapping_small =
        CreateCommitContext::new(&ctx, smallrepo.blob_repo(), vec![small_master_cs_id])
            .commit()
            .await?;
    let change_mapping_large =
        CreateCommitContext::new(&ctx, largerepo.blob_repo(), vec![large_master_cs_id])
            .commit()
            .await?;

    let commit_sync_repos = CommitSyncRepos::new(
        largerepo.blob_repo().clone(),
        smallrepo.blob_repo().clone(),
        &common_config,
    )?;

    let commit_syncer = CommitSyncer::new(
        &ctx,
        largerepo.synced_commit_mapping().clone(),
        commit_sync_repos,
        largerepo.live_commit_sync_config(),
        Arc::new(InProcessLease::new()),
    );

    update_mapping_with_version(
        &ctx,
        hashmap! {change_mapping_large => change_mapping_small},
        &commit_syncer,
        &new_version,
    )
    .await?;

    // After config change
    let second_large =
        CreateCommitContext::new(&ctx, largerepo.blob_repo(), vec![change_mapping_large])
            .add_file("prefix2/remapped_after", "content1")
            .add_file("not_remapped", "content2")
            .commit()
            .await?;

    let second_small = largerepo
        .xrepo_commit_lookup(&smallrepo, second_large, None)
        .await?;
    let file_changes: Vec<_> = second_small
        .unwrap()
        .id()
        .load(&ctx, smallrepo.blob_repo().blobstore())
        .await?
        .file_changes()
        .map(|(path, _)| path)
        .cloned()
        .collect();

    assert_eq!(file_changes, vec![MPath::new("remapped_after")?]);
    Ok(())
}

async fn init_x_repo(
    ctx: &CoreContext,
) -> Result<(Mononoke, TestLiveCommitSyncConfigSource), Error> {
    let (syncers, commit_sync_config, lv_cfg, lv_cfg_src) = init_small_large_repo(ctx).await?;

    let small_to_large = syncers.small_to_large;
    let mapping: ArcSyncedCommitMapping = Arc::new(small_to_large.get_mapping().clone());
    let mononoke = Mononoke::new_test_xrepo(
        ctx.clone(),
        (
            "smallrepo".to_string(),
            small_to_large.get_small_repo().clone(),
        ),
        (
            "largerepo".to_string(),
            small_to_large.get_large_repo().clone(),
        ),
        commit_sync_config.clone(),
        mapping.clone(),
        Arc::new(lv_cfg),
    )
    .await?;
    lv_cfg_src.add_config(commit_sync_config);
    Ok((mononoke, lv_cfg_src))
}

#[fbinit::test]
async fn resolve_changeset_id_prefix(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), Linear::getrepo(fb).await)],
    )
    .await?;

    let repo = mononoke
        .repo(ctx, "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let hg_cs_id = ChangesetSpecifier::Hg(HgChangesetId::from_str(
        "607314ef579bd2407752361ba1b0c1729d08b281",
    )?);

    let bonsai_cs_id = ChangesetSpecifier::Bonsai(ChangesetId::from_str(
        "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6",
    )?);

    // test different lengths
    let test_cases: Vec<(_, Vec<ChangesetPrefixSpecifier>)> = vec![
        (
            &hg_cs_id,
            vec![
                HgChangesetIdPrefix::from_str("6073")?.into(),
                HgChangesetIdPrefix::from_str("607314e")?.into(),
                HgChangesetIdPrefix::from_str("607314ef57")?.into(),
                HgChangesetIdPrefix::from_str("607314ef579bd2407752361ba")?.into(),
                HgChangesetIdPrefix::from_str("607314ef579bd2407752361ba1b0c1729d08b281")?.into(),
            ],
        ),
        (
            &bonsai_cs_id,
            vec![
                ChangesetIdPrefix::from_str("7785")?.into(),
                ChangesetIdPrefix::from_str("7785606")?.into(),
                ChangesetIdPrefix::from_str("7785606eb1f26f")?.into(),
                ChangesetIdPrefix::from_str("7785606eb1f26ff5722c831")?.into(),
                ChangesetIdPrefix::from_str(
                    "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6",
                )?
                .into(),
            ],
        ),
    ];

    for (expected, prefixes) in test_cases {
        for prefix in prefixes {
            assert_eq!(
                repo.resolve_changeset_id_prefix(prefix).await?,
                ChangesetSpecifierPrefixResolution::Single(*expected)
            );
        }
    }

    // nonexistent changeset
    assert_eq!(
        ChangesetSpecifierPrefixResolution::NoMatch,
        repo.resolve_changeset_id_prefix(HgChangesetIdPrefix::from_str("607314efffff")?.into())
            .await?
    );

    // invalid hex string
    assert!(HgChangesetIdPrefix::from_str("607314euuuuu").is_err());

    Ok(())
}

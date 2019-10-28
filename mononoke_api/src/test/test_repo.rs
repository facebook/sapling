/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::HashMap;
use std::str::FromStr;

use bytes::Bytes;
use chrono::{FixedOffset, TimeZone};
use failure::Error;
use fbinit::FacebookInit;
use fixtures::{branch_uneven, linear, many_files_dirs};
use futures::stream::Stream;
use futures_preview::compat::Future01CompatExt;
use futures_preview::future::{FutureExt, TryFutureExt};

use crate::{
    ChangesetId, ChangesetSpecifier, CoreContext, FileId, FileMetadata, FileType, HgChangesetId,
    Mononoke, TreeEntry, TreeId,
};
use mononoke_types::hash::{GitSha1, Sha1, Sha256};
use synced_commit_mapping::SyncedCommitMappingEntry;

#[fbinit::test]
fn commit_info_by_hash(fb: FacebookInit) -> Result<(), Error> {
    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(
        async move {
            let ctx = CoreContext::test_mock(fb);
            let mononoke =
                Mononoke::new_test(ctx.clone(), vec![("test".to_string(), linear::getrepo(fb))])
                    .await?;
            let repo = mononoke.repo(ctx, "test")?.expect("repo exists");
            let hash = "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6";
            let cs_id = ChangesetId::from_str(hash)?;
            let cs = repo
                .changeset(ChangesetSpecifier::Bonsai(cs_id))
                .await?
                .expect("changeset exists");

            assert_eq!(cs.message().await?, "modified 10");
            assert_eq!(cs.author().await?, "Jeremy Fitzhardinge <jsgf@fb.com>");
            assert_eq!(
                cs.author_date().await?,
                FixedOffset::west(7 * 3600).timestamp(1504041761, 0)
            );

            Ok(())
        }
            .boxed()
            .compat(),
    )
}

#[fbinit::test]
fn commit_info_by_hg_hash(fb: FacebookInit) -> Result<(), Error> {
    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(
        async move {
            let ctx = CoreContext::test_mock(fb);
            let mononoke =
                Mononoke::new_test(ctx.clone(), vec![("test".to_string(), linear::getrepo(fb))])
                    .await?;
            let repo = mononoke.repo(ctx, "test")?.expect("repo exists");
            let hg_hash = "607314ef579bd2407752361ba1b0c1729d08b281";
            let hg_cs_id = HgChangesetId::from_str(hg_hash)?;
            let cs = repo
                .changeset(ChangesetSpecifier::Hg(hg_cs_id))
                .await?
                .expect("changeset exists");

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
            .boxed()
            .compat(),
    )
}

#[fbinit::test]
fn commit_info_by_bookmark(fb: FacebookInit) -> Result<(), Error> {
    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(
        async move {
            let ctx = CoreContext::test_mock(fb);
            let mononoke =
                Mononoke::new_test(ctx.clone(), vec![("test".to_string(), linear::getrepo(fb))])
                    .await?;
            let repo = mononoke.repo(ctx, "test")?.expect("repo exists");
            let cs = repo
                .resolve_bookmark("master")
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
            .boxed()
            .compat(),
    )
}

#[fbinit::test]
fn commit_hg_changeset_ids(fb: FacebookInit) -> Result<(), Error> {
    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(
        async move {
            let ctx = CoreContext::test_mock(fb);
            let mononoke =
                Mononoke::new_test(ctx.clone(), vec![("test".to_string(), linear::getrepo(fb))])
                    .await?;
            let repo = mononoke.repo(ctx, "test")?.expect("repo exists");
            let hash1 = "2cb6d2d3052bfbdd6a95a61f2816d81130033b5f5a99e8d8fc24d9238d85bb48";
            let hash2 = "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6";
            let hg_hash1 = "607314ef579bd2407752361ba1b0c1729d08b281";
            let hg_hash2 = "79a13814c5ce7330173ec04d279bf95ab3f652fb";
            let ids: HashMap<_, _> = repo
                .changeset_hg_ids(vec![
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
            .boxed()
            .compat(),
    )
}

#[fbinit::test]
fn commit_is_ancestor_of(fb: FacebookInit) -> Result<(), Error> {
    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(
        async move {
            let ctx = CoreContext::test_mock(fb);
            let mononoke = Mononoke::new_test(
                ctx.clone(),
                vec![("test".to_string(), branch_uneven::getrepo(fb))],
            )
            .await?;
            let repo = mononoke.repo(ctx, "test")?.expect("repo exists");
            let mut changesets = Vec::new();
            for hg_hash in [
                "5d43888a3c972fe68c224f93d41b30e9f888df7c", // 0: branch 1 near top
                "d7542c9db7f4c77dab4b315edd328edf1514952f", // 1: branch 1 near bottom
                "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5", // 2: branch 2
                "15c40d0abc36d47fb51c8eaec51ac7aad31f669c", // 3: base
            ]
            .iter()
            {
                let changeset = repo
                    .changeset(ChangesetSpecifier::Hg(HgChangesetId::from_str(hg_hash)?))
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
            ]
            .iter()
            {
                assert_eq!(
                    changesets[*index]
                        .as_ref()
                        .unwrap()
                        .is_ancestor_of(changesets[*base_index].as_ref().unwrap().id())
                        .await?,
                    *is_ancestor_of,
                    "changesets[{}].is_ancestor_of(changesets[{}].id()) == {}",
                    *index,
                    *base_index,
                    *is_ancestor_of
                );
            }
            Ok(())
        }
            .boxed()
            .compat(),
    )
}

#[fbinit::test]
fn commit_path_exists_and_type(fb: FacebookInit) -> Result<(), Error> {
    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(
        async move {
            let ctx = CoreContext::test_mock(fb);
            let mononoke = Mononoke::new_test(
                ctx.clone(),
                vec![("test".to_string(), many_files_dirs::getrepo(fb))],
            )
            .await?;
            let repo = mononoke.repo(ctx, "test")?.expect("repo exists");
            let hash = "b0d1bf77898839595ee0f0cba673dd6e3be9dadaaa78bc6dd2dea97ca6bee77e";
            let cs_id = ChangesetId::from_str(hash)?;
            let cs = repo
                .changeset(ChangesetSpecifier::Bonsai(cs_id))
                .await?
                .expect("changeset exists");

            let root_path = cs.root();
            assert_eq!(root_path.exists().await?, true);
            assert_eq!(root_path.is_dir().await?, true);

            let dir1_path = cs.path("dir1")?;
            assert_eq!(dir1_path.exists().await?, true);
            assert_eq!(dir1_path.is_dir().await?, true);
            assert_eq!(dir1_path.file_type().await?, None);

            let file1_path = cs.path("dir1/file_1_in_dir1")?;
            assert_eq!(file1_path.exists().await?, true);
            assert_eq!(file1_path.is_dir().await?, false);
            assert_eq!(file1_path.file_type().await?, Some(FileType::Regular));

            let nonexistent_path = cs.path("nonexistent")?;
            assert_eq!(nonexistent_path.exists().await?, false);
            assert_eq!(nonexistent_path.is_dir().await?, false);
            assert_eq!(nonexistent_path.file_type().await?, None);

            Ok(())
        }
            .boxed()
            .compat(),
    )
}

#[fbinit::test]
fn tree_list(fb: FacebookInit) -> Result<(), Error> {
    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(
        async move {
            let ctx = CoreContext::test_mock(fb);
            let mononoke = Mononoke::new_test(
                ctx.clone(),
                vec![("test".to_string(), many_files_dirs::getrepo(fb))],
            )
            .await?;
            let repo = mononoke.repo(ctx, "test")?.expect("repo exists");
            let hash = "b0d1bf77898839595ee0f0cba673dd6e3be9dadaaa78bc6dd2dea97ca6bee77e";
            let cs_id = ChangesetId::from_str(hash)?;
            let cs = repo
                .changeset(ChangesetSpecifier::Bonsai(cs_id))
                .await?
                .expect("changeset exists");
            assert_eq!(
                {
                    let path = cs.root();
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
                    let path = cs.path("dir1")?;
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
                let path = cs.path("dir1/subdir1")?;
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
                    let path = cs.path("dir1/subdir1/subsubdir1")?;
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
            assert!(repo
                .tree(TreeId::from_bytes([1; 32]).unwrap())
                .await?
                .is_none());
            // Get tree by non-existent path returns None.
            {
                let path = cs.path("nonexistent")?;
                assert!(path.tree().await?.is_none());
            }

            Ok(())
        }
            .boxed()
            .compat(),
    )
}

#[fbinit::test]
fn file_metadata(fb: FacebookInit) -> Result<(), Error> {
    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(
        async move {
            let ctx = CoreContext::test_mock(fb);
            let mononoke = Mononoke::new_test(
                ctx.clone(),
                vec![("test".to_string(), many_files_dirs::getrepo(fb))],
            )
            .await?;
            let repo = mononoke.repo(ctx, "test")?.expect("repo exists");

            let expected_metadata = FileMetadata {
                content_id: FileId::from_str(
                    "9d9cf646b38852094ec48ab401eea6f4481cc89a80589331845dc08f75a652d2",
                )?,
                total_size: 9,
                sha1: Sha1::from_str("b29930dda02406077d96a7b7a08ce282b3de6961")?,
                sha256: Sha256::from_str(
                    "47d741b6059c6d7e99be25ce46fb9ba099cfd6515de1ef7681f93479d25996a4",
                )?,
                git_sha1: GitSha1::from_sha1(
                    Sha1::from_str("ac3e272b72bbf89def8657766b855d0656630ed4")?,
                    "blob",
                    9,
                ),
            };

            // Get file by changeset path.
            let hash = "b0d1bf77898839595ee0f0cba673dd6e3be9dadaaa78bc6dd2dea97ca6bee77e";
            let cs_id = ChangesetId::from_str(hash)?;
            let cs = repo
                .changeset(ChangesetSpecifier::Bonsai(cs_id))
                .await?
                .expect("changeset exists");

            let path = cs.path("dir1/file_1_in_dir1")?;
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
            .boxed()
            .compat(),
    )
}

#[fbinit::test]
fn file_contents(fb: FacebookInit) -> Result<(), Error> {
    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(
        async move {
            let ctx = CoreContext::test_mock(fb);
            let mononoke = Mononoke::new_test(
                ctx.clone(),
                vec![("test".to_string(), many_files_dirs::getrepo(fb))],
            )
            .await?;
            let repo = mononoke.repo(ctx, "test")?.expect("repo exists");

            let hash = "b0d1bf77898839595ee0f0cba673dd6e3be9dadaaa78bc6dd2dea97ca6bee77e";
            let cs_id = ChangesetId::from_str(hash)?;
            let cs = repo
                .changeset(ChangesetSpecifier::Bonsai(cs_id))
                .await?
                .expect("changeset exists");

            let path = cs.path("dir1/file_1_in_dir1")?;
            let file = path.file().await?.unwrap();
            let content = file.content().await.concat2().compat().await?;
            assert_eq!(content, Bytes::from("content1\n"));

            let content_range = file.content_range(3, 4).await.concat2().compat().await?;
            assert_eq!(content_range, Bytes::from("tent"));

            Ok(())
        }
            .boxed()
            .compat(),
    )
}

#[fbinit::test]
fn xrepo_commit_lookup(fb: FacebookInit) -> Result<(), Error> {
    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(
        async move {
            let ctx = CoreContext::test_mock(fb);
            let mononoke = Mononoke::new_test(
                ctx.clone(),
                vec![
                    ("test".to_string(), linear::getrepo(fb)),
                    ("test2".to_string(), many_files_dirs::getrepo(fb)),
                ],
            )
            .await?;
            let repo1 = mononoke.repo(ctx.clone(), "test")?.expect("repo exists");
            let repo2 = mononoke.repo(ctx.clone(), "test2")?.expect("repo exists");

            let hash1 = ChangesetId::from_str(
                "2cb6d2d3052bfbdd6a95a61f2816d81130033b5f5a99e8d8fc24d9238d85bb48",
            )?;

            // Confirm that a cross-repo lookup for an unsynced commit just fails
            let cs = repo1
                .xrepo_commit_lookup(&repo2, ChangesetSpecifier::Bonsai(hash1))
                .await?;
            assert!(cs.is_none());

            // Now insert a remapping to a commit that exists, and see that work
            let hash2 = ChangesetId::from_str(
                "b0d1bf77898839595ee0f0cba673dd6e3be9dadaaa78bc6dd2dea97ca6bee77e",
            )?;
            let entry = SyncedCommitMappingEntry::new(
                repo1.blob_repo().get_repoid(),
                hash1,
                repo2.blob_repo().get_repoid(),
                hash2,
            );
            assert!(
                repo1
                    .synced_commit_mapping()
                    .add(ctx.clone(), entry)
                    .compat()
                    .await?
            );
            let cs = repo1
                .xrepo_commit_lookup(&repo2, ChangesetSpecifier::Bonsai(hash1))
                .await?
                .expect("changeset exists");
            assert_eq!(cs.id(), hash2);

            Ok(())
        }
            .boxed()
            .compat(),
    )
}

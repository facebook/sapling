/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::BTreeMap;
use std::convert::TryFrom;

use anyhow::Error;
use assert_matches::assert_matches;
use bytes::Bytes;
use chrono::{FixedOffset, TimeZone};
use fbinit::FacebookInit;
use fixtures::{linear, many_files_dirs};

use futures::stream::Stream;
use futures_preview::compat::Future01CompatExt;
use futures_preview::future::{FutureExt, TryFutureExt};

use crate::{
    ChangesetContext, ChangesetId, CoreContext, CreateChange, FileType, Mononoke, MononokeError,
    MononokePath, RepoWriteContext,
};

#[fbinit::test]
fn create_commit(fb: FacebookInit) -> Result<(), Error> {
    let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
    runtime.block_on(
        async move {
            let ctx = CoreContext::test_mock(fb);
            let mononoke =
                Mononoke::new_test(ctx.clone(), vec![("test".to_string(), linear::getrepo(fb))])
                    .await?;
            let repo = mononoke
                .repo(ctx, "test")?
                .expect("repo exists")
                .write()
                .await?;
            let expected_hash = "68c9120f387cf1c3b7e4c2e30cdbd5b953f27a732cfe9f42f335f0091ece3c6c";
            let parent_hash = "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6";
            let parents = vec![ChangesetId::from_str(parent_hash)?];
            let author = String::from("Test Author <test@example.com>");
            let author_date = FixedOffset::east(0).ymd(2000, 2, 1).and_hms(12, 0, 0);
            let committer = None;
            let committer_date = None;
            let message = String::from("Test Created Commit");
            let extra = BTreeMap::new();
            let mut changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
            changes.insert(
                MononokePath::try_from("TEST_CREATE")?,
                CreateChange::NewContent(Bytes::from("TEST CREATE\n"), FileType::Regular, None),
            );

            let cs = repo
                .create_changeset(
                    parents,
                    author,
                    author_date,
                    committer,
                    committer_date,
                    message,
                    extra,
                    changes,
                )
                .await?;

            assert_eq!(cs.message().await?, "Test Created Commit");
            assert_eq!(cs.id(), ChangesetId::from_str(expected_hash)?);

            let content = cs
                .path("TEST_CREATE")?
                .file()
                .await?
                .expect("file should exist")
                .content()
                .await
                .collect()
                .compat()
                .await?;
            assert_eq!(content, vec![Bytes::from("TEST CREATE\n")]);

            Ok(())
        }
            .boxed()
            .compat(),
    )
}

#[fbinit::test]
fn create_commit_bad_changes(fb: FacebookInit) -> Result<(), Error> {
    let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
    runtime.block_on(
        async move {
            let ctx = CoreContext::test_mock(fb);
            let mononoke = Mononoke::new_test(
                ctx.clone(),
                vec![("test".to_string(), many_files_dirs::getrepo(fb))],
            )
            .await?;
            let repo = mononoke
                .repo(ctx, "test")?
                .expect("repo exists")
                .write()
                .await?;

            async fn create_changeset(
                repo: &RepoWriteContext,
                changes: BTreeMap<MononokePath, CreateChange>,
            ) -> Result<ChangesetContext, MononokeError> {
                let parent_hash =
                    "b0d1bf77898839595ee0f0cba673dd6e3be9dadaaa78bc6dd2dea97ca6bee77e";
                let parents = vec![ChangesetId::from_str(parent_hash)?];
                let author = String::from("Test Author <test@example.com>");
                let author_date = FixedOffset::east(0).ymd(2000, 2, 1).and_hms(12, 0, 0);
                let committer = None;
                let committer_date = None;
                let message = String::from("Test Created Commit");
                let extra = BTreeMap::new();
                repo.create_changeset(
                    parents,
                    author,
                    author_date,
                    committer,
                    committer_date,
                    message,
                    extra,
                    changes,
                )
                .await
            }

            // Cannot delete a file that is not there
            let mut changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
            changes.insert(MononokePath::try_from("TEST_CREATE")?, CreateChange::Delete);
            assert_matches!(
                create_changeset(&repo, changes).await,
                Err(MononokeError::InvalidRequest(_))
            );

            // Cannot replace a file with a directory without deleting the file
            let mut changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
            changes.insert(
                MononokePath::try_from("1/TEST_CREATE")?,
                CreateChange::NewContent(Bytes::from("test"), FileType::Regular, None),
            );
            assert_matches!(
                create_changeset(&repo, changes.clone()).await,
                Err(MononokeError::InvalidRequest(_))
            );

            // Deleting the file means we can now replace it with a directory.
            changes.insert(MononokePath::try_from("1")?, CreateChange::Delete);
            assert!(create_changeset(&repo, changes).await.is_ok());

            // Changes cannot introduce path conflicts
            let mut changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
            changes.insert(
                MononokePath::try_from("TEST_CREATE")?,
                CreateChange::NewContent(Bytes::from("test"), FileType::Regular, None),
            );
            changes.insert(
                MononokePath::try_from("TEST_CREATE/TEST_CREATE")?,
                CreateChange::NewContent(Bytes::from("test"), FileType::Regular, None),
            );
            assert_matches!(
                create_changeset(&repo, changes).await,
                Err(MononokeError::InvalidRequest(_))
            );

            // Superfluous changes when a directory is replaced by a file are dropped
            let mut changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
            changes.insert(
                MononokePath::try_from("dir1")?,
                CreateChange::NewContent(Bytes::from("test"), FileType::Regular, None),
            );
            let cs1 = create_changeset(&repo, changes.clone()).await?;

            changes.insert(
                MononokePath::try_from("dir1/file_1_in_dir1")?,
                CreateChange::Delete,
            );
            changes.insert(
                MononokePath::try_from("dir1/subdir1/file_1")?,
                CreateChange::Delete,
            );
            let cs2 = create_changeset(&repo, changes).await?;

            // Since the superfluous changes were dropped, the two commits
            // have the same bonsai hash.
            assert_eq!(cs1.id(), cs2.id());

            Ok(())
        }
            .boxed()
            .compat(),
    )
}

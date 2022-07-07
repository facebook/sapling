/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use anyhow::Error;
use assert_matches::assert_matches;
use blobrepo::BlobRepo;
use bytes::Bytes;
use chrono::FixedOffset;
use chrono::TimeZone;
use derived_data_utils::derived_data_utils;
use fbinit::FacebookInit;
use fixtures::Linear;
use fixtures::ManyFilesDirs;
use fixtures::TestRepoFixture;
use futures::try_join;
use std::str::FromStr;

use crate::ChangesetContext;
use crate::ChangesetId;
use crate::CoreContext;
use crate::CreateChange;
use crate::CreateChangeFile;
use crate::FileType;
use crate::Mononoke;
use crate::MononokeError;
use crate::MononokePath;
use crate::RepoContext;

#[fbinit::test]
async fn test_create_commit(fb: FacebookInit) -> Result<(), Error> {
    create_commit(fb, "skeleton_manifests").await?;

    Ok(())
}

// Check that commits were created correctly, and also check that only a single
// derived data type was derived (i.e. check that we don't derive something that we aren't supposed
// to).
async fn create_commit(fb: FacebookInit, derived_data_to_derive: &str) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), Linear::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
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
    let bubble = None;
    let mut changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
    changes.insert(
        MononokePath::try_from("TEST_CREATE")?,
        CreateChange::Tracked(
            CreateChangeFile::New {
                bytes: Bytes::from("TEST CREATE\n"),
                file_type: FileType::Regular,
            },
            None,
        ),
    );

    let cs = repo
        .create_changeset(
            parents,
            author.clone(),
            author_date,
            committer.clone(),
            committer_date,
            message.clone(),
            extra.clone(),
            changes.clone(),
            bubble,
        )
        .await?;

    changes.insert(
        MononokePath::try_from("TEST_CREATE")?,
        CreateChange::Tracked(
            CreateChangeFile::New {
                bytes: Bytes::from("TEST CREATE2\n"),
                file_type: FileType::Regular,
            },
            None,
        ),
    );
    let second_cs = repo
        .create_changeset(
            vec![cs.id()],
            author,
            author_date,
            committer,
            committer_date,
            message,
            extra,
            changes,
            bubble,
        )
        .await?;

    validate_unnecessary_derived_data_is_not_derived(
        &ctx,
        repo.blob_repo(),
        cs.id(),
        second_cs.id(),
        derived_data_to_derive,
    )
    .await?;

    assert_eq!(cs.message().await?, "Test Created Commit");
    assert_eq!(cs.id(), ChangesetId::from_str(expected_hash)?);

    let content = cs
        .path_with_content("TEST_CREATE")?
        .file()
        .await?
        .expect("file should exist")
        .content_concat()
        .await?;
    assert_eq!(content, Bytes::from("TEST CREATE\n"));

    let content = second_cs
        .path_with_content("TEST_CREATE")?
        .file()
        .await?
        .expect("file should exist")
        .content_concat()
        .await?;
    assert_eq!(content, Bytes::from("TEST CREATE2\n"));

    Ok(())
}

// We expect that after creating a commit only derived a single specific derived data
// type is derived for a parent changeset, and none derived for the newly created changeset.
// This function validates it's actualy the case
async fn validate_unnecessary_derived_data_is_not_derived(
    ctx: &CoreContext,
    repo: &BlobRepo,
    parent_cs_id: ChangesetId,
    cs_id: ChangesetId,
    derived_data_to_derive: &str,
) -> Result<(), Error> {
    for ty in &repo.get_active_derived_data_types_config().types {
        if ty == "git_trees" {
            // Derived data utils doesn't support git_trees, so we have to skip it
            continue;
        }
        let utils = derived_data_utils(ctx.fb, repo, ty)?;
        let not_derived = utils
            .pending(ctx.clone(), repo.clone(), vec![parent_cs_id, cs_id])
            .await?;
        // It's expected to derive skeleton manifests for the parent commit
        if ty == derived_data_to_derive {
            assert_eq!(not_derived, vec![cs_id]);
        } else {
            assert_eq!(not_derived, vec![parent_cs_id, cs_id]);
        }
    }

    Ok(())
}

#[fbinit::test]
async fn create_commit_bad_changes(fb: FacebookInit) -> Result<(), Error> {
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

    async fn create_changeset(
        repo: &RepoContext,
        changes: BTreeMap<MononokePath, CreateChange>,
    ) -> Result<ChangesetContext, MononokeError> {
        let parent_hash = "b0d1bf77898839595ee0f0cba673dd6e3be9dadaaa78bc6dd2dea97ca6bee77e";
        let parents = vec![ChangesetId::from_str(parent_hash)?];
        let author = String::from("Test Author <test@example.com>");
        let author_date = FixedOffset::east(0).ymd(2000, 2, 1).and_hms(12, 0, 0);
        let committer = None;
        let committer_date = None;
        let message = String::from("Test Created Commit");
        let extra = BTreeMap::new();
        let bubble = None;
        repo.create_changeset(
            parents,
            author,
            author_date,
            committer,
            committer_date,
            message,
            extra,
            changes,
            bubble,
        )
        .await
    }

    // Cannot delete a file that is not there
    let mut changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
    changes.insert(
        MononokePath::try_from("TEST_CREATE")?,
        CreateChange::Deletion,
    );
    assert_matches!(
        create_changeset(&repo, changes).await,
        Err(MononokeError::InvalidRequest(_))
    );

    // Cannot replace a file with a directory without deleting the file
    let mut changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
    changes.insert(
        MononokePath::try_from("1/TEST_CREATE")?,
        CreateChange::Tracked(
            CreateChangeFile::New {
                bytes: Bytes::from("test"),
                file_type: FileType::Regular,
            },
            None,
        ),
    );
    assert_matches!(
        create_changeset(&repo, changes.clone()).await,
        Err(MononokeError::InvalidRequest(_))
    );

    // Deleting the file means we can now replace it with a directory.
    changes.insert(MononokePath::try_from("1")?, CreateChange::Deletion);
    assert!(create_changeset(&repo, changes).await.is_ok());

    // Changes cannot introduce path conflicts
    let mut changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
    changes.insert(
        MononokePath::try_from("TEST_CREATE")?,
        CreateChange::Tracked(
            CreateChangeFile::New {
                bytes: Bytes::from("test"),
                file_type: FileType::Regular,
            },
            None,
        ),
    );
    changes.insert(
        MononokePath::try_from("TEST_CREATE/TEST_CREATE")?,
        CreateChange::Tracked(
            CreateChangeFile::New {
                bytes: Bytes::from("test"),
                file_type: FileType::Regular,
            },
            None,
        ),
    );
    assert_matches!(
        create_changeset(&repo, changes).await,
        Err(MononokeError::InvalidRequest(_))
    );

    // Superfluous changes when a directory is replaced by a file are dropped
    let mut changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
    changes.insert(
        MononokePath::try_from("dir1")?,
        CreateChange::Tracked(
            CreateChangeFile::New {
                bytes: Bytes::from("test"),
                file_type: FileType::Regular,
            },
            None,
        ),
    );
    let cs1 = create_changeset(&repo, changes.clone()).await?;

    changes.insert(
        MononokePath::try_from("dir1/file_1_in_dir1")?,
        CreateChange::Deletion,
    );
    changes.insert(
        MononokePath::try_from("dir1/subdir1/file_1")?,
        CreateChange::Deletion,
    );
    let cs2 = create_changeset(&repo, changes).await?;

    // Since the superfluous changes were dropped, the two commits
    // have the same bonsai hash.
    assert_eq!(cs1.id(), cs2.id());

    Ok(())
}

#[fbinit::test]
async fn test_create_merge_commit(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), Linear::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    async fn create_changeset(
        repo: &RepoContext,
        changes: BTreeMap<MononokePath, CreateChange>,
        parents: Vec<ChangesetId>,
    ) -> Result<ChangesetContext, MononokeError> {
        let author = String::from("Test Author <test@example.com>");
        let author_date = FixedOffset::east(0).ymd(2000, 2, 1).and_hms(12, 0, 0);
        let committer = None;
        let committer_date = None;
        let message = String::from("Test Created Commit");
        let extra = BTreeMap::new();
        let bubble = None;
        repo.create_changeset(
            parents,
            author.clone(),
            author_date,
            committer.clone(),
            committer_date,
            message.clone(),
            extra.clone(),
            changes.clone(),
            bubble,
        )
        .await
    }

    let initial_hash = "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6";
    let initial_parents = vec![ChangesetId::from_str(initial_hash)?];
    let mut p1_changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
    p1_changes.insert(
        MononokePath::try_from("TEST_CREATE_p1")?,
        CreateChange::Tracked(
            CreateChangeFile::New {
                bytes: Bytes::from("TEST CREATE\n"),
                file_type: FileType::Regular,
            },
            None,
        ),
    );
    let mut p2_changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
    p2_changes.insert(
        MononokePath::try_from("TEST_CREATE_p2")?,
        CreateChange::Tracked(
            CreateChangeFile::New {
                bytes: Bytes::from("TEST CREATE\n"),
                file_type: FileType::Regular,
            },
            None,
        ),
    );

    let (p1, p2) = try_join!(
        create_changeset(&repo, p1_changes, initial_parents.clone(),),
        create_changeset(&repo, p2_changes, initial_parents)
    )?;

    let mut merge_changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
    merge_changes.insert(
        MononokePath::try_from("TEST_MERGE")?,
        CreateChange::Tracked(
            CreateChangeFile::New {
                bytes: Bytes::from("TEST CREATE\n"),
                file_type: FileType::Regular,
            },
            None,
        ),
    );
    create_changeset(&repo, merge_changes, vec![p1.id(), p2.id()]).await?;

    let mut merge_changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
    merge_changes.insert(
        MononokePath::try_from("TEST_CREATE_p1")?,
        CreateChange::Tracked(
            CreateChangeFile::New {
                bytes: Bytes::from("TEST MERGE OVERRIDE\n"),
                file_type: FileType::Regular,
            },
            None,
        ),
    );
    create_changeset(&repo, merge_changes, vec![p1.id(), p2.id()]).await?;

    Ok(())
}

#[fbinit::test]
async fn test_merge_commit_parent_file_conflict(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), Linear::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    async fn create_changeset(
        repo: &RepoContext,
        changes: BTreeMap<MononokePath, CreateChange>,
        parents: Vec<ChangesetId>,
    ) -> Result<ChangesetContext, MononokeError> {
        let author = String::from("Test Author <test@example.com>");
        let author_date = FixedOffset::east(0).ymd(2000, 2, 1).and_hms(12, 0, 0);
        let committer = None;
        let committer_date = None;
        let message = String::from("Test Created Commit");
        let extra = BTreeMap::new();
        let bubble = None;
        repo.create_changeset(
            parents,
            author.clone(),
            author_date,
            committer.clone(),
            committer_date,
            message.clone(),
            extra.clone(),
            changes.clone(),
            bubble,
        )
        .await
    }

    let initial_hash = "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6";
    let initial_parents = vec![ChangesetId::from_str(initial_hash)?];
    let mut p1_changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
    p1_changes.insert(
        MononokePath::try_from("TEST_FILE")?,
        CreateChange::Tracked(
            CreateChangeFile::New {
                bytes: Bytes::from("TEST CREATE_p1\n"),
                file_type: FileType::Regular,
            },
            None,
        ),
    );
    let mut p2_changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
    p2_changes.insert(
        MononokePath::try_from("TEST_CREATE")?,
        CreateChange::Tracked(
            CreateChangeFile::New {
                bytes: Bytes::from("TEST CREATE_p2\n"),
                file_type: FileType::Regular,
            },
            None,
        ),
    );

    let mut p3_changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
    p3_changes.insert(
        MononokePath::try_from("TEST_FILE")?,
        CreateChange::Tracked(
            CreateChangeFile::New {
                bytes: Bytes::from("TEST CREATE_p3\n"),
                file_type: FileType::Regular,
            },
            None,
        ),
    );

    let (p1, p2, p3) = try_join!(
        create_changeset(&repo, p1_changes, initial_parents.clone()),
        create_changeset(&repo, p2_changes, initial_parents.clone()),
        create_changeset(&repo, p3_changes, initial_parents.clone())
    )?;

    // p1 and p2 do not conflict
    create_changeset(&repo, BTreeMap::new(), vec![p1.id(), p2.id()]).await?;

    // p1 and p3 do conflict
    assert!(
        create_changeset(&repo, BTreeMap::new(), vec![p1.id(), p3.id()])
            .await
            .is_err()
    );

    // Can't hide it by making p3 third parent, or by moving p1 to second parent and p3 as third
    assert!(
        create_changeset(&repo, BTreeMap::new(), vec![p1.id(), p2.id(), p3.id()])
            .await
            .is_err()
    );
    assert!(
        create_changeset(&repo, BTreeMap::new(), vec![p2.id(), p1.id(), p3.id()])
            .await
            .is_err()
    );

    Ok(())
}

#[fbinit::test]
async fn test_merge_commit_parent_tree_file_conflict(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), Linear::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    async fn create_changeset(
        repo: &RepoContext,
        changes: BTreeMap<MononokePath, CreateChange>,
        parents: Vec<ChangesetId>,
    ) -> Result<ChangesetContext, MononokeError> {
        let author = String::from("Test Author <test@example.com>");
        let author_date = FixedOffset::east(0).ymd(2000, 2, 1).and_hms(12, 0, 0);
        let committer = None;
        let committer_date = None;
        let message = String::from("Test Created Commit");
        let extra = BTreeMap::new();
        let bubble = None;
        repo.create_changeset(
            parents,
            author.clone(),
            author_date,
            committer.clone(),
            committer_date,
            message.clone(),
            extra.clone(),
            changes.clone(),
            bubble,
        )
        .await
    }

    let initial_hash = "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6";
    let initial_parents = vec![ChangesetId::from_str(initial_hash)?];
    let mut p1_changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
    p1_changes.insert(
        MononokePath::try_from("TEST_FILE/REALLY_A_DIR")?,
        CreateChange::Tracked(
            CreateChangeFile::New {
                bytes: Bytes::from("TEST CREATE_p1\n"),
                file_type: FileType::Regular,
            },
            None,
        ),
    );
    let mut p2_changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
    p2_changes.insert(
        MononokePath::try_from("TEST_CREATE")?,
        CreateChange::Tracked(
            CreateChangeFile::New {
                bytes: Bytes::from("TEST CREATE_p2\n"),
                file_type: FileType::Regular,
            },
            None,
        ),
    );

    let mut p3_changes: BTreeMap<MononokePath, CreateChange> = BTreeMap::new();
    p3_changes.insert(
        MononokePath::try_from("TEST_FILE")?,
        CreateChange::Tracked(
            CreateChangeFile::New {
                bytes: Bytes::from("TEST CREATE_p3\n"),
                file_type: FileType::Regular,
            },
            None,
        ),
    );

    let (p1, p2, p3) = try_join!(
        create_changeset(&repo, p1_changes, initial_parents.clone()),
        create_changeset(&repo, p2_changes, initial_parents.clone()),
        create_changeset(&repo, p3_changes, initial_parents.clone())
    )?;

    // p1 and p2 do not conflict
    create_changeset(&repo, BTreeMap::new(), vec![p1.id(), p2.id()]).await?;

    // p1 and p3 do conflict
    assert!(
        create_changeset(&repo, BTreeMap::new(), vec![p1.id(), p3.id()])
            .await
            .is_err()
    );

    // Can't hide it by making p3 third parent, or by moving p1 to second parent and p3 as third
    assert!(
        create_changeset(&repo, BTreeMap::new(), vec![p1.id(), p2.id(), p3.id()])
            .await
            .is_err()
    );
    assert!(
        create_changeset(&repo, BTreeMap::new(), vec![p2.id(), p1.id(), p3.id()])
            .await
            .is_err()
    );

    Ok(())
}

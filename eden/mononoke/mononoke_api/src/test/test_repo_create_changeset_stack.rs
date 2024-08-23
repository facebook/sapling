/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::str::FromStr;

use anyhow::Context;
use anyhow::Error;
use bytes::Bytes;
use chrono::FixedOffset;
use chrono::TimeZone;
use fbinit::FacebookInit;
use fixtures::Linear;
use fixtures::TestRepoFixture;
use maplit::btreemap;
use mononoke_macros::mononoke;
use mononoke_types::path::MPath;

use crate::ChangesetContext;
use crate::ChangesetId;
use crate::CoreContext;
use crate::CreateChange;
use crate::CreateChangeFile;
use crate::CreateCopyInfo;
use crate::CreateInfo;
use crate::Mononoke;
use crate::MononokeError;
use crate::MononokeRepo;
use crate::RepoContext;

async fn create_changeset_stack<R: MononokeRepo>(
    repo: &RepoContext<R>,
    changes_stack: Vec<BTreeMap<MPath, CreateChange>>,
    stack_parents: Vec<ChangesetId>,
) -> Result<Vec<ChangesetContext<R>>, MononokeError> {
    let author = String::from("Test Author <test@example.com>");
    let author_date = FixedOffset::east_opt(0)
        .unwrap()
        .with_ymd_and_hms(2000, 2, 1, 12, 0, 0)
        .unwrap();
    let committer = None;
    let committer_date = None;
    let extra = BTreeMap::new();
    let bubble = None;
    let git_extra_headers = None;
    let info_stack = (1..=changes_stack.len())
        .map(|n| CreateInfo {
            author: author.clone(),
            author_date,
            committer: committer.clone(),
            committer_date,
            message: format!("Test Created Commit {n}"),
            extra: extra.clone(),
            git_extra_headers: git_extra_headers.clone(),
        })
        .collect::<Vec<_>>();
    Ok(repo
        .create_changeset_stack(stack_parents, info_stack, changes_stack, bubble)
        .await?
        .into_iter()
        .map(|(_hg_extra, cs)| cs)
        .collect())
}

async fn create_changesets_sequentially<R: MononokeRepo>(
    repo: &RepoContext<R>,
    changes_stack: Vec<BTreeMap<MPath, CreateChange>>,
    stack_parents: Vec<ChangesetId>,
) -> Result<Vec<ChangesetContext<R>>, MononokeError> {
    let author = String::from("Test Author <test@example.com>");
    let author_date = FixedOffset::east_opt(0)
        .unwrap()
        .with_ymd_and_hms(2000, 2, 1, 12, 0, 0)
        .unwrap();
    let committer = None;
    let committer_date = None;
    let extra = BTreeMap::new();
    let bubble = None;
    let git_extra_headers = None;
    let mut parents = stack_parents;
    let mut change_num = 1;
    let mut result = Vec::new();
    for changes in changes_stack {
        let info = CreateInfo {
            author: author.clone(),
            author_date,
            committer: committer.clone(),
            committer_date,
            message: format!("Test Created Commit {change_num}"),
            extra: extra.clone(),
            git_extra_headers: git_extra_headers.clone(),
        };
        let (_hg_extra, commit) = repo
            .create_changeset(parents, info, changes, bubble)
            .await?;
        parents = vec![commit.id()];
        result.push(commit);
        change_num += 1;
    }
    Ok(result)
}

async fn compare_create_stack<R: MononokeRepo>(
    stack_repo: &RepoContext<R>,
    seq_repo: &RepoContext<R>,
    changes_stack: Vec<BTreeMap<MPath, CreateChange>>,
    stack_parents: Vec<ChangesetId>,
) -> Result<Option<Vec<ChangesetContext<R>>>, Error> {
    let stack =
        create_changeset_stack(stack_repo, changes_stack.clone(), stack_parents.clone()).await;
    let seq = create_changesets_sequentially(seq_repo, changes_stack, stack_parents).await;

    match (stack, seq) {
        (Err(_), Err(_)) => Ok(None),
        (_, Err(e)) => Err(e).context("Create failed only on sequential create"),
        (Err(e), _) => Err(e).context("Create failed only on stacked create"),
        (Ok(stack), Ok(seq)) => {
            let stack_ids: Vec<_> = stack.iter().map(|c| c.id()).collect();
            let seq_ids: Vec<_> = seq.iter().map(|c| c.id()).collect();
            assert_eq!(
                stack_ids, seq_ids,
                "stack creation and sequential creation gave different commits"
            );
            Ok(Some(stack))
        }
    }
}

#[mononoke::fbinit_test]
async fn test_create_commit_stack(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(vec![
        ("test_stack".to_string(), Linear::get_repo(fb).await),
        ("test_seq".to_string(), Linear::get_repo(fb).await),
    ])
    .await?;
    let stack_repo = mononoke
        .repo(ctx.clone(), "test_stack")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let seq_repo = mononoke
        .repo(ctx.clone(), "test_seq")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let initial_hash = "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6";
    let initial_parents = vec![ChangesetId::from_str(initial_hash)?];

    let changes = vec![
        btreemap! {
            MPath::try_from("TEST_FILE")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("TEST CREATE 1\n"),
                None,
            ),
        },
        btreemap! {
            MPath::try_from("TEST_FILE")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("TEST CHANGE 1\n"),
                None,
            ),
            MPath::try_from("TEST_DIR/TEST_FILE")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("TEST CREATE 2\n"),
                None,
            ),
        },
        btreemap! {
            MPath::try_from("TEST_DIR/TEST_FILE")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("TEST CHANGE 2\n"),
                None,
            ),
        },
    ];

    let stack = compare_create_stack(&stack_repo, &seq_repo, changes, initial_parents.clone())
        .await?
        .expect("stack should have been created");

    for (commit, path, content) in [
        (0, "TEST_FILE", "TEST CREATE 1\n"),
        (1, "TEST_FILE", "TEST CHANGE 1\n"),
        (2, "TEST_FILE", "TEST CHANGE 1\n"),
        (1, "TEST_DIR/TEST_FILE", "TEST CREATE 2\n"),
        (2, "TEST_DIR/TEST_FILE", "TEST CHANGE 2\n"),
    ] {
        let actual_content = stack[commit]
            .path_with_content(path)
            .await?
            .file()
            .await?
            .expect("file should exist")
            .content_concat()
            .await?;
        assert_eq!(actual_content, Bytes::from(content));
    }
    assert!(
        stack[0]
            .path_with_content("TEST_DIR/TEST_FILE")
            .await?
            .file()
            .await?
            .is_none(),
        "file should not exist yet"
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_create_commit_stack_delete_files(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(vec![
        ("test_stack".to_string(), Linear::get_repo(fb).await),
        ("test_seq".to_string(), Linear::get_repo(fb).await),
    ])
    .await?;
    let stack_repo = mononoke
        .repo(ctx.clone(), "test_stack")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let seq_repo = mononoke
        .repo(ctx.clone(), "test_seq")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let initial_hash = "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6";
    let initial_parents = vec![ChangesetId::from_str(initial_hash)?];

    // Deleting a file that doesn't exist should fail.
    let changes = vec![
        btreemap! {
            MPath::try_from("TEST_FILE")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("TEST CREATE 1\n"),
                None,
            ),
        },
        btreemap! {
            MPath::try_from("TEST_OTHER_FILE")? =>
            CreateChange::Deletion,
        },
    ];
    assert!(
        compare_create_stack(&stack_repo, &seq_repo, changes, initial_parents.clone())
            .await?
            .is_none()
    );

    // But succeed if the file was created in the stack.
    let changes = vec![
        btreemap! {
            MPath::try_from("TEST_NEW_FILE")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("TEST CREATE 1\n"),
                None,
            ),
        },
        btreemap! {
            MPath::try_from("TEST_NEW_FILE")? =>
            CreateChange::Deletion,
        },
    ];
    let _stack = compare_create_stack(&stack_repo, &seq_repo, changes, initial_parents.clone())
        .await?
        .expect("stack should have been created");

    // Deleting a file twice in the stack should also fail.
    let changes = vec![
        btreemap! {
            MPath::try_from("TEST_FILE")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("TEST CREATE 1\n"),
                None,
            ),
        },
        btreemap! {
            MPath::try_from("TEST_FILE")? =>
            CreateChange::Deletion,
        },
        btreemap! {
            MPath::try_from("TEST_FILE")? =>
            CreateChange::Deletion,
        },
    ];
    assert!(
        compare_create_stack(&stack_repo, &seq_repo, changes, initial_parents.clone())
            .await?
            .is_none()
    );

    // This should also be true if the first deletion was implicit.
    let changes = vec![
        btreemap! {
            MPath::try_from("TEST_PATH/SUBDIR/TEST_FILE")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("TEST CREATE 3\n"),
                None,
            ),
        },
        btreemap! {
            MPath::try_from("TEST_PATH")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("TEST CREATE 3\n"),
                None,
            ),
        },
        btreemap! {
            MPath::try_from("TEST_PATH/SUBDIR/TEST_FILE")? =>
            CreateChange::Deletion,
        },
    ];
    assert!(
        compare_create_stack(&stack_repo, &seq_repo, changes, initial_parents.clone())
            .await?
            .is_none()
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_create_commit_stack_path_conflicts(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(vec![
        ("test_stack".to_string(), Linear::get_repo(fb).await),
        ("test_seq".to_string(), Linear::get_repo(fb).await),
    ])
    .await?;
    let stack_repo = mononoke
        .repo(ctx.clone(), "test_stack")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let seq_repo = mononoke
        .repo(ctx.clone(), "test_seq")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let initial_hash = "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6";
    let initial_parents = vec![ChangesetId::from_str(initial_hash)?];

    // Attempting to create path conflicts in a stack should fail
    let changes = vec![
        btreemap! {
            MPath::try_from("TEST_PATH")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("TEST CREATE 1\n"),
                None,
            ),
        },
        btreemap! {
            MPath::try_from("TEST_PATH/SUBDIR/TEST_FILE")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("TEST CREATE 1\n"),
                None,
            ),
        },
    ];
    assert!(
        compare_create_stack(&stack_repo, &seq_repo, changes, initial_parents.clone())
            .await?
            .is_none()
    );

    // But succeeds if you resolve the path conflict
    let changes = vec![
        btreemap! {
            MPath::try_from("TEST_PATH")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("TEST CREATE 1\n"),
                None,
            ),
        },
        btreemap! {
            MPath::try_from("TEST_PATH")? =>
            CreateChange::Deletion,
            MPath::try_from("TEST_PATH/SUBDIR/TEST_FILE")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("TEST CREATE 1\n"),
                None,
            ),
        },
    ];
    let _stack = compare_create_stack(&stack_repo, &seq_repo, changes, initial_parents.clone())
        .await?
        .expect("stack should have been created");

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_create_commit_stack_copy_from(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(vec![
        ("test_stack".to_string(), Linear::get_repo(fb).await),
        ("test_seq".to_string(), Linear::get_repo(fb).await),
    ])
    .await?;
    let stack_repo = mononoke
        .repo(ctx.clone(), "test_stack")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let seq_repo = mononoke
        .repo(ctx.clone(), "test_seq")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let initial_hash = "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6";
    let initial_parents = vec![ChangesetId::from_str(initial_hash)?];

    // Copy from source must exist in the parent
    let mut changes = vec![
        btreemap! {
            MPath::try_from("TEST_FILE")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("TEST CREATE 1\n"),
                None,
            ),
        },
        btreemap! {
            MPath::try_from("TEST_FILE2")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("TEST CREATE 1\n"),
                Some(CreateCopyInfo::new(MPath::try_from("OTHER_FILE")?, 0)),
            ),
        },
    ];
    assert!(
        compare_create_stack(
            &stack_repo,
            &seq_repo,
            changes.clone(),
            initial_parents.clone()
        )
        .await?
        .is_none()
    );

    // It's ok if it was created earlier in the stack
    changes.insert(
        0,
        btreemap! {
            MPath::try_from("OTHER_FILE")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("TEST CREATE 2\n"),
                None,
            ),
        },
    );
    let _stack = compare_create_stack(&stack_repo, &seq_repo, changes, initial_parents.clone())
        .await?
        .expect("stack should have been created");

    Ok(())
}

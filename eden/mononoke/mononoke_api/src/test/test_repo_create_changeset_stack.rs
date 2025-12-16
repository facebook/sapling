/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

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
use mononoke_types::FileType;
use mononoke_types::path::MPath;

use crate::ChangesetContext;
use crate::ChangesetId;
use crate::CoreContext;
use crate::CreateChange;
use crate::CreateChangeFile;
use crate::CreateChangeFileContents;
use crate::CreateChangesetCheckMode;
use crate::CreateChangesetChecks;
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
    create_changeset_stack_impl(
        repo,
        changes_stack,
        stack_parents,
        CreateChangesetChecks {
            noop_file_changes: CreateChangesetCheckMode::Check,
            deleted_files_existed_in_a_parent: CreateChangesetCheckMode::Check,
            empty_changeset: CreateChangesetCheckMode::Check,
        },
    )
    .await
}

async fn create_changeset_stack_fix_request<R: MononokeRepo>(
    repo: &RepoContext<R>,
    changes_stack: Vec<BTreeMap<MPath, CreateChange>>,
    stack_parents: Vec<ChangesetId>,
) -> Result<Vec<ChangesetContext<R>>, MononokeError> {
    create_changeset_stack_impl(
        repo,
        changes_stack,
        stack_parents,
        CreateChangesetChecks {
            noop_file_changes: CreateChangesetCheckMode::Fix,
            deleted_files_existed_in_a_parent: CreateChangesetCheckMode::Fix,
            empty_changeset: CreateChangesetCheckMode::Fix,
        },
    )
    .await
}

async fn create_changeset_stack_impl<R: MononokeRepo>(
    repo: &RepoContext<R>,
    changes_stack: Vec<BTreeMap<MPath, CreateChange>>,
    stack_parents: Vec<ChangesetId>,
    checks: CreateChangesetChecks,
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
        .create_changeset_stack(stack_parents, info_stack, changes_stack, bubble, checks)
        .await?
        .into_iter()
        .map(|created_changeset| created_changeset.changeset_ctx)
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
        let commit = repo
            .create_changeset(
                parents,
                info,
                changes,
                bubble,
                CreateChangesetChecks {
                    noop_file_changes: CreateChangesetCheckMode::Check,
                    deleted_files_existed_in_a_parent: CreateChangesetCheckMode::Check,
                    empty_changeset: CreateChangesetCheckMode::Check,
                },
            )
            .await?
            .changeset_ctx;
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

/// Helper to get file type from a changeset
async fn get_file_type<R: MononokeRepo>(
    changeset: &ChangesetContext<R>,
    path: &str,
) -> Result<Option<FileType>, Error> {
    let path_ctx = changeset.path_with_content(path).await?;
    Ok(path_ctx.file_type().await?)
}

#[mononoke::fbinit_test]
async fn test_create_commit_stack(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (test_stack_repo, commits, _) = Linear::get_repo_and_dag(fb).await;
    let mononoke = Mononoke::new_test(vec![
        ("test_stack".to_string(), test_stack_repo),
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

    let initial_parents = vec![commits["K"]];

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
    let (test_stack_repo, commits, _) = Linear::get_repo_and_dag(fb).await;
    let mononoke = Mononoke::new_test(vec![
        ("test_stack".to_string(), test_stack_repo),
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

    let initial_parents = vec![commits["K"]];

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
            MPath::try_from("EXTRA")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("extra\n"),
                None,
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

    // If we specify CreateChangesetCheckMode::Fix for the "deleted_files_existed_in_parents"
    // check, then the creation request succeeds and the noop deletion is removed
    assert!(
        create_changeset_stack_fix_request(&stack_repo, changes, initial_parents.clone())
            .await?
            .get(1)
            .unwrap()
            .file_changes()
            .await?
            .len()
            == 1
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
async fn test_create_commit_stack_noop_file_changes_check(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);

    let (test_stack_repo, commits, _) = Linear::get_repo_and_dag(fb).await;
    let mononoke = Mononoke::new_test(vec![
        ("test_stack".to_string(), test_stack_repo),
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

    // Noop file changes that don't change file content should fail
    let changes = vec![btreemap! {
        MPath::try_from("10")? =>
        CreateChange::Tracked(
            CreateChangeFile::new_regular("modified10\n"),
            None,
        ),
        MPath::try_from("EXTRA")? =>
        CreateChange::Tracked(
            CreateChangeFile::new_regular("extra\n"),
            None,
        ),
    }];
    assert!(
        compare_create_stack(&stack_repo, &seq_repo, changes.clone(), vec![commits["K"]])
            .await?
            .is_none()
    );

    // If we specify CreateChangesetCheckMode::Fix for the noop check then the
    // creation request succeeds and the noop file change is removed
    assert!(
        create_changeset_stack_fix_request(&stack_repo, changes, vec![commits["K"]])
            .await?
            .first()
            .unwrap()
            .file_changes()
            .await?
            .len()
            == 1
    );

    // Noop file changes that don't change file content should fail
    let changes = vec![btreemap! {
        MPath::try_from("10")? =>
        CreateChange::Tracked(
            CreateChangeFile::new_regular("modified10\n"),
            None,
        ),
    }];
    assert!(
        compare_create_stack(&stack_repo, &seq_repo, changes.clone(), vec![commits["K"]])
            .await?
            .is_none()
    );

    // The commit still fails the empty_changeset check as it becomes empty after removing
    // the no-op file change
    assert!(
        create_changeset_stack_fix_request(&stack_repo, changes, vec![commits["K"]])
            .await
            .is_err()
    );

    // Noop file changes for files introduced in the stack should fail
    let changes = vec![
        btreemap! {
            MPath::try_from("NEW_PATH")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("NEW_CONTENT\n"),
                None,
            )
        },
        btreemap! {
            MPath::try_from("NEW_PATH")? =>
            CreateChange::Tracked(
                CreateChangeFile::new_regular("NEW_CONTENT\n"),
                None,
            ),
        },
    ];
    assert!(
        compare_create_stack(&stack_repo, &seq_repo, changes, vec![commits["K"]])
            .await?
            .is_none()
    );

    // File changes that resolve merge conflicts are not no-op changes
    // and should succeed
    let changes = vec![btreemap! {
        MPath::try_from("MERGE_CONFLICT_PATH")? =>
        CreateChange::Tracked(
            CreateChangeFile::new_regular("CONTENT_P1\n"),
            None,
        )
    }];
    let p1 = compare_create_stack(&stack_repo, &seq_repo, changes, vec![commits["K"]])
        .await?
        .unwrap()
        .pop()
        .unwrap()
        .id();

    let changes = vec![btreemap! {
        MPath::try_from("MERGE_CONFLICT_PATH")? =>
        CreateChange::Tracked(
            CreateChangeFile::new_regular("CONTENT_P2\n"),
            None,
        )
    }];
    let p2 = compare_create_stack(&stack_repo, &seq_repo, changes, vec![commits["K"]])
        .await?
        .unwrap()
        .pop()
        .unwrap()
        .id();

    let changes = vec![btreemap! {
        MPath::try_from("MERGE_CONFLICT_PATH")? =>
        CreateChange::Tracked(
            CreateChangeFile::new_regular("CONTENT_P1\n"),
            None,
        ),
    }];
    assert!(
        compare_create_stack(&stack_repo, &seq_repo, changes, vec![p1, p2])
            .await?
            .is_some()
    );

    // If the file content is the same across all parents and the new
    // commit doesn't change the file content, it's a noop change
    let changes = vec![btreemap! {
        MPath::try_from("NEW_PATH")? =>
        CreateChange::Tracked(
            CreateChangeFile::new_regular("CONTENT\n"),
            None,
        )
    }];
    let p1 = compare_create_stack(&stack_repo, &seq_repo, changes, vec![commits["K"]])
        .await?
        .unwrap()
        .pop()
        .unwrap()
        .id();

    let changes = vec![btreemap! {
        MPath::try_from("NEW_PATH")? =>
        CreateChange::Tracked(
            CreateChangeFile::new_regular("CONTENT\n"),
            None,
        )
    }];
    let p2 = compare_create_stack(&stack_repo, &seq_repo, changes, vec![commits["K"]])
        .await?
        .unwrap()
        .pop()
        .unwrap()
        .id();

    let changes = vec![btreemap! {
        MPath::try_from("NEW_PATH")? =>
        CreateChange::Tracked(
            CreateChangeFile::new_regular("CONTENT\n"),
            None,
        ),
    }];
    assert!(
        compare_create_stack(&stack_repo, &seq_repo, changes, vec![p1, p2])
            .await?
            .is_none()
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_create_commit_stack_path_conflicts(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);

    let (test_stack_repo, commits, _) = Linear::get_repo_and_dag(fb).await;
    let mononoke = Mononoke::new_test(vec![
        ("test_stack".to_string(), test_stack_repo),
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

    let initial_parents = vec![commits["K"]];

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
    let (test_stack_repo, commits, _) = Linear::get_repo_and_dag(fb).await;
    let mononoke = Mononoke::new_test(vec![
        ("test_stack".to_string(), test_stack_repo),
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

    let initial_parents = vec![commits["K"]];

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

/// Test that file_type: None inherits from the parent commit
#[mononoke::fbinit_test]
async fn test_create_changeset_file_type_inheritance_from_parent(
    fb: FacebookInit,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (test_stack_repo, commits, _) = Linear::get_repo_and_dag(fb).await;
    let mononoke = Mononoke::new_test(vec![("test".to_string(), test_stack_repo)]).await?;
    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let parent_id = commits["K"];

    // First, create a parent commit with an executable file
    let create_exec_changes = vec![btreemap! {
        MPath::try_from("exec_file")? =>
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("executable content\n"),
                },
                file_type: Some(FileType::Executable),
                git_lfs: None,
            },
            None,
        ),
    }];
    let exec_parent = create_changeset_stack(&repo, create_exec_changes, vec![parent_id])
        .await?
        .pop()
        .expect("commit created");

    // Verify parent has executable type
    assert_eq!(
        get_file_type(&exec_parent, "exec_file").await?,
        Some(FileType::Executable)
    );

    // Now modify the file with file_type: None - should inherit EXEC
    let modify_changes = vec![btreemap! {
        MPath::try_from("exec_file")? =>
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("modified executable content\n"),
                },
                file_type: None, // Should inherit from parent
                git_lfs: None,
            },
            None,
        ),
    }];
    let child = create_changeset_stack(&repo, modify_changes, vec![exec_parent.id()])
        .await?
        .pop()
        .expect("commit created");

    // Verify inherited executable type
    assert_eq!(
        get_file_type(&child, "exec_file").await?,
        Some(FileType::Executable),
        "file_type: None should inherit Executable from parent"
    );

    Ok(())
}

/// Test that file_type: None defaults to Regular for new files
#[mononoke::fbinit_test]
async fn test_create_changeset_file_type_defaults_to_regular(
    fb: FacebookInit,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (test_stack_repo, commits, _) = Linear::get_repo_and_dag(fb).await;
    let mononoke = Mononoke::new_test(vec![("test".to_string(), test_stack_repo)]).await?;
    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let parent_id = commits["K"];

    // Create a new file with file_type: None
    let changes = vec![btreemap! {
        MPath::try_from("new_file")? =>
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("new file content\n"),
                },
                file_type: None, // Should default to Regular
                git_lfs: None,
            },
            None,
        ),
    }];
    let commit = create_changeset_stack(&repo, changes, vec![parent_id])
        .await?
        .pop()
        .expect("commit created");

    // Verify defaults to Regular
    assert_eq!(
        get_file_type(&commit, "new_file").await?,
        Some(FileType::Regular),
        "file_type: None should default to Regular for new files"
    );

    Ok(())
}

/// Test that file_type: None inherits from the copy source
#[mononoke::fbinit_test]
async fn test_create_changeset_file_type_copy_chain_inheritance(
    fb: FacebookInit,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (test_stack_repo, commits, _) = Linear::get_repo_and_dag(fb).await;
    let mononoke = Mononoke::new_test(vec![("test".to_string(), test_stack_repo)]).await?;
    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let parent_id = commits["K"];

    // First, create a parent with an executable file
    let create_exec_changes = vec![btreemap! {
        MPath::try_from("source_exec")? =>
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("executable source\n"),
                },
                file_type: Some(FileType::Executable),
                git_lfs: None,
            },
            None,
        ),
    }];
    let exec_parent = create_changeset_stack(&repo, create_exec_changes, vec![parent_id])
        .await?
        .pop()
        .expect("commit created");

    // Copy the file with file_type: None - should inherit EXEC from copy source
    let copy_changes = vec![btreemap! {
        MPath::try_from("copied_file")? =>
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("copied content\n"),
                },
                file_type: None, // Should inherit from copy source
                git_lfs: None,
            },
            Some(CreateCopyInfo::new(MPath::try_from("source_exec")?, 0)),
        ),
    }];
    let commit = create_changeset_stack(&repo, copy_changes, vec![exec_parent.id()])
        .await?
        .pop()
        .expect("commit created");

    // Verify inherited executable type from copy source
    assert_eq!(
        get_file_type(&commit, "copied_file").await?,
        Some(FileType::Executable),
        "file_type: None should inherit Executable from copy source"
    );

    Ok(())
}

/// Test that file_type: None inherits correctly within a stack
#[mononoke::fbinit_test]
async fn test_create_changeset_stack_file_type_inheritance_within_stack(
    fb: FacebookInit,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (test_stack_repo, commits, _) = Linear::get_repo_and_dag(fb).await;
    let mononoke = Mononoke::new_test(vec![("test".to_string(), test_stack_repo)]).await?;
    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let parent_id = commits["K"];

    // Create a stack where:
    // - Commit 1: creates exec file
    // - Commit 2: modifies with file_type: None (should inherit EXEC)
    let changes = vec![
        btreemap! {
            MPath::try_from("stack_file")? =>
            CreateChange::Tracked(
                CreateChangeFile {
                    contents: CreateChangeFileContents::New {
                        bytes: Bytes::from("initial exec content\n"),
                    },
                    file_type: Some(FileType::Executable),
                    git_lfs: None,
                },
                None,
            ),
        },
        btreemap! {
            MPath::try_from("stack_file")? =>
            CreateChange::Tracked(
                CreateChangeFile {
                    contents: CreateChangeFileContents::New {
                        bytes: Bytes::from("modified content\n"),
                    },
                    file_type: None, // Should inherit from commit 1
                    git_lfs: None,
                },
                None,
            ),
        },
    ];

    let stack = create_changeset_stack(&repo, changes, vec![parent_id]).await?;

    // Verify first commit has executable type
    assert_eq!(
        get_file_type(&stack[0], "stack_file").await?,
        Some(FileType::Executable)
    );

    // Verify second commit inherits executable type
    assert_eq!(
        get_file_type(&stack[1], "stack_file").await?,
        Some(FileType::Executable),
        "file_type: None in stack should inherit from previous commit in stack"
    );

    Ok(())
}

/// Test that explicit file_type overrides parent's type
#[mononoke::fbinit_test]
async fn test_create_changeset_file_type_explicit_override(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (test_stack_repo, commits, _) = Linear::get_repo_and_dag(fb).await;
    let mononoke = Mononoke::new_test(vec![("test".to_string(), test_stack_repo)]).await?;
    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    let parent_id = commits["K"];

    // First, create a parent with an executable file
    let create_exec_changes = vec![btreemap! {
        MPath::try_from("typed_file")? =>
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("executable content\n"),
                },
                file_type: Some(FileType::Executable),
                git_lfs: None,
            },
            None,
        ),
    }];
    let exec_parent = create_changeset_stack(&repo, create_exec_changes, vec![parent_id])
        .await?
        .pop()
        .expect("commit created");

    // Verify parent has executable type
    assert_eq!(
        get_file_type(&exec_parent, "typed_file").await?,
        Some(FileType::Executable)
    );

    // Modify with explicit Regular type - should override EXEC
    let modify_changes = vec![btreemap! {
        MPath::try_from("typed_file")? =>
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("now regular content\n"),
                },
                file_type: Some(FileType::Regular), // Explicit override
                git_lfs: None,
            },
            None,
        ),
    }];
    let child = create_changeset_stack(&repo, modify_changes, vec![exec_parent.id()])
        .await?
        .pop()
        .expect("commit created");

    // Verify explicit Regular type overrides parent's Executable
    assert_eq!(
        get_file_type(&child, "typed_file").await?,
        Some(FileType::Regular),
        "explicit file_type: Regular should override parent's Executable"
    );

    Ok(())
}

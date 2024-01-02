/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Error;
use async_trait::async_trait;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::TryFutureExt;
use hook_manager::ChangesetHook;
use hook_manager::CrossRepoPushSource;
use hook_manager::FileChange as FileDiff;
use hook_manager::HookExecution;
use hook_manager::HookFileContentProvider;
use hook_manager::HookManager;
use hook_manager::HookRejectionInfo;
use hook_manager::PathContent;
use hook_manager::PushAuthoredBy;
use maplit::hashmap;
use metaconfig_types::HookManagerParams;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use mononoke_types_mocks::contentid::ONES_CTID;
use mononoke_types_mocks::contentid::THREES_CTID;
use mononoke_types_mocks::contentid::TWOS_CTID;
use permission_checker::InternalAclProvider;
use regex::Regex;
use repo_blobstore::RepoBlobstoreRef;
use scuba_ext::MononokeScubaSampleBuilder;
use sorted_vector_map::sorted_vector_map;
use tests_utils::bookmark;
use tests_utils::BasicTestRepo;
use tests_utils::CreateCommitContext;

use crate::RepoHookFileContentProvider;

#[derive(Clone)]
struct FindFilesChangesetHook {
    pub filename: String,
}

#[async_trait]
impl ChangesetHook for FindFilesChangesetHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        _bookmark: &BookmarkKey,
        _changeset: &'cs BonsaiChangeset,
        content_manager: &'fetcher dyn HookFileContentProvider,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        let path = to_mpath(self.filename.as_str());
        let res = content_manager
            .find_content(ctx, BookmarkKey::new("master")?, vec![path.clone()])
            .await;

        match res {
            Ok(contents) => Ok(match contents.get(&path) {
                Some(PathContent::File(_)) => HookExecution::Accepted,
                _ => HookExecution::Rejected(HookRejectionInfo::new("there is no such file")),
            }),
            Err(err) => {
                if err.to_string().contains("Bookmark master does not exist") {
                    return Ok(HookExecution::Rejected(HookRejectionInfo::new(
                        "no master bookmark found",
                    )));
                }
                Err(Error::from(err))
            }
        }
    }
}

#[derive(Clone)]
struct FileChangesChangesetHook {
    pub added: i32,
    pub changed: i32,
    pub removed: i32,
}

#[async_trait]
impl ChangesetHook for FileChangesChangesetHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        content_manager: &'fetcher dyn HookFileContentProvider,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        let parent = changeset.parents().next();
        let (added, changed, removed) = if let Some(parent) = parent {
            let file_changes = content_manager
                .file_changes(ctx, changeset.get_changeset_id(), parent)
                .await?;

            let (mut added, mut changed, mut removed) = (0, 0, 0);
            for (_path, change) in file_changes.into_iter() {
                match change {
                    FileDiff::Added(_) => added += 1,
                    FileDiff::Changed(_, _) => changed += 1,
                    FileDiff::Removed => removed += 1,
                }
            }
            Result::<_, Error>::Ok((added, changed, removed))
        } else {
            Ok((0, 0, 0))
        }?;

        if added != self.added || changed != self.changed || removed != self.removed {
            return Ok(HookExecution::Rejected(HookRejectionInfo::new(
                "Wrong number of added, changed or removed files",
            )));
        }

        Ok(HookExecution::Accepted)
    }
}

#[derive(Clone)]
struct LatestChangesChangesetHook(HashMap<NonRootMPath, Option<ChangesetId>>);

#[async_trait]
impl ChangesetHook for LatestChangesChangesetHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        _bookmark: &BookmarkKey,
        _changeset: &'cs BonsaiChangeset,
        content_manager: &'fetcher dyn HookFileContentProvider,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        let paths = self.0.keys().cloned().collect();
        let res = content_manager
            .latest_changes(ctx, BookmarkKey::new("master")?, paths)
            .map_err(Error::from)
            .await?;

        for (path, linknode) in self.0.iter() {
            let found_linknode = res.get(path).map(|info| info.changeset_id());
            if linknode.as_ref() != found_linknode {
                return Ok(HookExecution::Rejected(HookRejectionInfo::new(
                    "found linknode doesn't match the expected one",
                )));
            }
        }
        Ok(HookExecution::Accepted)
    }
}

#[fbinit::test]
async fn test_cs_find_content_hook_with_blob_store(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BasicTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
    let root_id = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/file", "dir/file")
        .add_file("dir-2/file", "dir-2/file")
        .commit()
        .await?;
    let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![root_id])
        .add_file("dir/sub/file", "dir/sub/file")
        .add_file("dir-2", "dir-2 is a file now")
        .commit()
        .await?;

    // find simple file
    let hook_name1 = "hook1".to_string();
    let hook1 = Box::new(FindFilesChangesetHook {
        filename: "dir/sub/file".to_string(),
    });

    // find non-existent file
    let hook_name2 = "hook2".to_string();
    let hook2 = Box::new(FindFilesChangesetHook {
        filename: "dir-2/file".to_string(),
    });

    // run first hook on a repo without master bookmark
    // the hook should reject the commit
    let hooks: HashMap<String, Box<dyn ChangesetHook>> = hashmap! {
        hook_name1.clone() => hook1.clone() as Box<dyn ChangesetHook>,
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec![hook_name1.clone()]
    };
    let regexes = hashmap! {};
    let expected = hashmap! {
        hook_name1.clone() => HookExecution::Rejected(HookRejectionInfo::new("no master bookmark found")),
    };

    run_changeset_hooks_with_mgr(
        ctx.clone(),
        &repo,
        None,
        "bm1",
        hooks,
        bookmarks,
        regexes.clone(),
        expected,
    )
    .await;

    // set master bookmark
    let mut txn = repo.bookmarks().create_transaction(ctx.clone());
    txn.force_set(
        &BookmarkKey::new("master")?,
        bcs_id,
        BookmarkUpdateReason::TestMove,
    )?;
    txn.commit().await?;

    // run hooks again
    let hooks: HashMap<String, Box<dyn ChangesetHook>> = hashmap! {
        hook_name1.clone() => hook1 as Box<dyn ChangesetHook>,
        hook_name2.clone() => hook2 as Box<dyn ChangesetHook>,
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec![hook_name1.clone(), hook_name2.clone()]
    };
    let regexes = hashmap! {};
    let expected = hashmap! {
        hook_name1 => HookExecution::Accepted,
        hook_name2 => HookExecution::Rejected(HookRejectionInfo::new("there is no such file")),
    };
    run_changeset_hooks_with_mgr(
        ctx,
        &repo,
        None,
        "bm1",
        hooks,
        bookmarks,
        regexes.clone(),
        expected,
    )
    .await;

    Ok(())
}

#[fbinit::test]
async fn test_cs_file_changes_hook_with_blob_store(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BasicTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
    let root_id = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file", "file")
        .add_file("dir/file", "dir/file")
        .add_file("dir/sub/file", "dir/sub/file")
        .add_file("dir-2/file", "dir-2/file")
        .commit()
        .await?;
    // set master bookmark
    bookmark(&ctx, &repo, "master").set_to(root_id).await?;

    let bcs_id = CreateCommitContext::new(&ctx, &repo, vec![root_id])
        .delete_file("file")
        .add_file("dir", "dir to file")
        .add_file("dir-2/file", "updated dir-2/file")
        .add_file("dir-3/sub/file-1", "dir-3/sub/file-1")
        .add_file("dir-3/sub/file-2", "dir-3/sub/file-2")
        .commit()
        .await?;
    let changeset = bcs_id.load(&ctx, repo.repo_blobstore()).await?;

    let hook_name = "hook".to_string();
    let hook = Box::new(FileChangesChangesetHook {
        added: 3,
        changed: 1,
        removed: 3,
    });

    let hooks: HashMap<String, Box<dyn ChangesetHook>> = hashmap! {
        hook_name.clone() => hook as Box<dyn ChangesetHook>,
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec![hook_name.clone()]
    };
    let regexes = hashmap! {};
    let expected = hashmap! {
        hook_name => HookExecution::Accepted,
    };
    run_changeset_hooks_with_mgr(
        ctx,
        &repo,
        Some(changeset),
        "bm1",
        hooks,
        bookmarks,
        regexes.clone(),
        expected,
    )
    .await;

    Ok(())
}

#[fbinit::test]
async fn test_cs_latest_changes_hook_with_blob_store(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BasicTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
    let root_id = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file", "file")
        .commit()
        .await?;
    // set master bookmark
    bookmark(&ctx, &repo, "master").set_to(root_id).await?;

    let hook_name = "hook".to_string();
    let expected = hashmap! { to_mpath("file") => Some(root_id), to_mpath("non_existent") => None };
    let hook = Box::new(LatestChangesChangesetHook(expected));

    let hooks: HashMap<String, Box<dyn ChangesetHook>> = hashmap! {
        hook_name.clone() => hook as Box<dyn ChangesetHook>,
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec![hook_name.clone()]
    };
    let regexes = hashmap! {};
    let expected = hashmap! {
        hook_name => HookExecution::Accepted,
    };
    run_changeset_hooks_with_mgr(
        ctx,
        &repo,
        None,
        "bm1",
        hooks,
        bookmarks,
        regexes.clone(),
        expected,
    )
    .await;

    Ok(())
}

async fn run_changeset_hooks_with_mgr(
    ctx: CoreContext,
    repo: &BasicTestRepo,
    changeset: Option<BonsaiChangeset>,
    bookmark_name: &str,
    hooks: HashMap<String, Box<dyn ChangesetHook>>,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
    expected: HashMap<String, HookExecution>,
) {
    let mut hook_manager = setup_hook_manager(ctx.fb, repo, bookmarks, regexes).await;
    for (hook_name, hook) in hooks {
        hook_manager.register_changeset_hook(&hook_name, hook, Default::default());
    }

    let changeset = changeset.unwrap_or_else(default_changeset);
    let res = hook_manager
        .run_hooks_for_bookmark(
            &ctx,
            vec![changeset].iter(),
            &BookmarkKey::new(bookmark_name).unwrap(),
            None,
            CrossRepoPushSource::NativeToThisRepo,
            PushAuthoredBy::User,
        )
        .await
        .unwrap();
    let map: HashMap<String, HookExecution> = res
        .into_iter()
        .map(|outcome| (outcome.get_hook_name().to_string(), outcome.into()))
        .collect();
    assert_eq!(expected, map);
}

async fn setup_hook_manager(
    fb: FacebookInit,
    repo: &BasicTestRepo,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
) -> HookManager {
    let mut hook_manager = hook_manager_repo(fb, repo).await;
    for (bookmark_name, hook_names) in bookmarks {
        hook_manager
            .set_hooks_for_bookmark(BookmarkKey::new(bookmark_name).unwrap().into(), hook_names);
    }
    for (regx, hook_names) in regexes {
        hook_manager.set_hooks_for_bookmark(Regex::new(&regx).unwrap().into(), hook_names);
    }
    hook_manager
}

fn default_changeset() -> BonsaiChangeset {
    BonsaiChangesetMut {
        parents: Vec::new(),
        author: "Jeremy Fitzhardinge <jsgf@fb.com>".to_string(),
        author_date: DateTime::from_timestamp(1584887580, 0).expect("Getting timestamp"),
        committer: None,
        committer_date: None,
        message: "This is a commit message".to_string(),
        hg_extra: Default::default(),
        git_extra_headers: None,
        git_tree_hash: None,
        file_changes: sorted_vector_map!{
            to_mpath("dir1/subdir1/subsubdir1/file_1") => FileChange::tracked(ONES_CTID, FileType::Symlink, 15, None),
            to_mpath("dir1/subdir1/subsubdir2/file_1") => FileChange::tracked(TWOS_CTID, FileType::Regular, 17, None),
            to_mpath("dir1/subdir1/subsubdir2/file_2") => FileChange::tracked(THREES_CTID, FileType::Regular, 2, None),
        },
        is_snapshot: false,
        git_annotated_tag: None,
    }.freeze().expect("Created changeset")
}

async fn hook_manager_repo(fb: FacebookInit, repo: &BasicTestRepo) -> HookManager {
    let ctx = CoreContext::test_mock(fb);

    let content_manager = RepoHookFileContentProvider::new(repo);
    HookManager::new(
        ctx.fb,
        &InternalAclProvider::default(),
        Box::new(content_manager),
        HookManagerParams {
            disable_acl_checker: true,
            ..Default::default()
        },
        MononokeScubaSampleBuilder::with_discard(),
        "zoo".to_string(),
    )
    .await
    .expect("Failed to construct HookManager")
}

fn to_mpath(string: &str) -> NonRootMPath {
    NonRootMPath::new(string).unwrap()
}

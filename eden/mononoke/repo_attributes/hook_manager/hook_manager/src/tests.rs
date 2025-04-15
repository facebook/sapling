/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::hash_map::Entry;
use std::sync::Arc;

use anyhow::Error;
use async_trait::async_trait;
use bookmarks_types::BookmarkKey;
use context::CoreContext;
use fbinit::FacebookInit;
use maplit::hashmap;
use maplit::hashset;
use metaconfig_types::HookManagerParams;
use mononoke_macros::mononoke;
use mononoke_types::BasicFileChange;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ContentId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::GitLfs;
use mononoke_types::NonRootMPath;
use mononoke_types_mocks::contentid::FIVES_CTID;
use mononoke_types_mocks::contentid::FOURS_CTID;
use mononoke_types_mocks::contentid::ONES_CTID;
use mononoke_types_mocks::contentid::SIXES_CTID;
use mononoke_types_mocks::contentid::THREES_CTID;
use mononoke_types_mocks::contentid::TWOS_CTID;
use permission_checker::InternalAclProvider;
use regex::Regex;
use repo_permission_checker::NeverAllowRepoPermissionChecker;
use scuba_ext::MononokeScubaSampleBuilder;
use sorted_vector_map::sorted_vector_map;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookExecution;
use crate::HookManager;
use crate::HookRejectionInfo;
use crate::HookRepo;
use crate::PushAuthoredBy;

#[derive(Clone, Debug)]
struct FnChangesetHook {
    f: fn() -> HookExecution,
}

impl FnChangesetHook {
    fn new(f: fn() -> HookExecution) -> FnChangesetHook {
        FnChangesetHook { f }
    }
}

#[async_trait]
impl ChangesetHook for FnChangesetHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _repo: &'repo HookRepo,
        _bookmark: &BookmarkKey,
        _changeset: &'cs BonsaiChangeset,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        Ok((self.f)())
    }
}

fn always_accepting_changeset_hook() -> Box<dyn ChangesetHook> {
    let f: fn() -> HookExecution = || HookExecution::Accepted;
    Box::new(FnChangesetHook::new(f))
}

fn always_rejecting_changeset_hook() -> Box<dyn ChangesetHook> {
    let f: fn() -> HookExecution = default_rejection;
    Box::new(FnChangesetHook::new(f))
}

#[derive(Clone, Debug)]
struct ContentIdMatchingChangesetHook {
    expected_content_ids: HashMap<NonRootMPath, Option<ContentId>>,
}

#[async_trait]
impl ChangesetHook for ContentIdMatchingChangesetHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _repo: &'repo HookRepo,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        for (path, change) in changeset.simplified_file_changes() {
            // If we have a change to a path, but no expected change, fail
            let expected_content = match self.expected_content_ids.get(path) {
                Some(expected) => expected,
                None => return Ok(default_rejection()),
            };

            if change.map(|change| change.content_id()) != *expected_content {
                return Ok(default_rejection());
            }
        }

        Ok(HookExecution::Accepted)
    }
}

fn content_id_matching_changeset_hook(
    expected_content_ids: HashMap<NonRootMPath, Option<ContentId>>,
) -> Box<dyn ChangesetHook> {
    Box::new(ContentIdMatchingChangesetHook {
        expected_content_ids,
    })
}

#[derive(Clone, Debug)]
struct FnFileHook {
    f: fn() -> HookExecution,
}

impl FnFileHook {
    fn new(f: fn() -> HookExecution) -> FnFileHook {
        FnFileHook { f }
    }
}

#[async_trait]
impl FileHook for FnFileHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'repo: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _repo: &'repo HookRepo,
        _change: Option<&'change BasicFileChange>,
        _path: &'path NonRootMPath,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        Ok((self.f)())
    }
}

fn always_accepting_file_hook() -> Box<dyn FileHook> {
    let f: fn() -> HookExecution = || HookExecution::Accepted;
    Box::new(FnFileHook::new(f))
}

fn always_rejecting_file_hook() -> Box<dyn FileHook> {
    let f: fn() -> HookExecution = default_rejection;
    Box::new(FnFileHook::new(f))
}

#[derive(Clone, Debug)]
struct PathMatchingFileHook {
    paths: HashSet<NonRootMPath>,
}

#[async_trait]
impl FileHook for PathMatchingFileHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'repo: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _repo: &'repo HookRepo,
        _change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        Ok(if self.paths.contains(path) {
            HookExecution::Accepted
        } else {
            default_rejection()
        })
    }
}

fn path_matching_file_hook(paths: HashSet<NonRootMPath>) -> Box<dyn FileHook> {
    Box::new(PathMatchingFileHook { paths })
}

#[derive(Clone, Debug)]
struct ContentIdMatchingFileHook {
    expected_content_id: Option<ContentId>,
}

#[async_trait]
impl FileHook for ContentIdMatchingFileHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'repo: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _repo: &'repo HookRepo,
        change: Option<&'change BasicFileChange>,
        _path: &'path NonRootMPath,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        if change.map(|change| change.content_id()) == self.expected_content_id {
            Ok(HookExecution::Accepted)
        } else {
            Ok(default_rejection())
        }
    }
}

fn content_id_matching_file_hook(expected_content_id: Option<ContentId>) -> Box<dyn FileHook> {
    Box::new(ContentIdMatchingFileHook {
        expected_content_id,
    })
}

#[derive(Clone, Debug)]
struct IsSymLinkMatchingFileHook {
    is_symlink: bool,
}

#[async_trait]
impl FileHook for IsSymLinkMatchingFileHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'repo: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _repo: &'repo HookRepo,
        change: Option<&'change BasicFileChange>,
        _path: &'path NonRootMPath,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        let is_symlink = match change {
            Some(change) => change.file_type() == FileType::Symlink,
            None => false,
        };
        Ok(if self.is_symlink == is_symlink {
            HookExecution::Accepted
        } else {
            default_rejection()
        })
    }
}

fn is_symlink_matching_file_hook(is_symlink: bool) -> Box<dyn FileHook> {
    Box::new(IsSymLinkMatchingFileHook { is_symlink })
}

async fn setup_hook_manager(
    fb: FacebookInit,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
) -> HookManager {
    let ctx = CoreContext::test_mock(fb);
    let repo = test_repo_factory::build_empty(ctx.fb)
        .await
        .expect("Failed to construct repo");

    let mut hook_manager = HookManager::new(
        ctx.fb,
        &InternalAclProvider::default(),
        repo,
        HookManagerParams {
            disable_acl_checker: true,
            ..Default::default()
        },
        Arc::new(NeverAllowRepoPermissionChecker {}),
        MononokeScubaSampleBuilder::with_discard(),
        "zoo".to_string(),
    )
    .await
    .expect("Failed to construct HookManager");

    for (bookmark_name, hook_names) in bookmarks {
        hook_manager
            .set_hooks_for_bookmark(BookmarkKey::new(bookmark_name).unwrap().into(), hook_names);
    }
    for (regx, hook_names) in regexes {
        hook_manager.set_hooks_for_bookmark(Regex::new(&regx).unwrap().into(), hook_names);
    }
    hook_manager
}

fn default_rejection() -> HookExecution {
    HookExecution::Rejected(HookRejectionInfo::new_long("desc", "long_desc".to_string()))
}

fn to_mpath(string: &str) -> NonRootMPath {
    NonRootMPath::new(string).unwrap()
}

fn default_changeset() -> BonsaiChangeset {
    BonsaiChangesetMut {
        author: "Jeremy Fitzhardinge <jsgf@fb.com>".to_string(),
        author_date: DateTime::from_timestamp(1584887580, 0).expect("Getting timestamp"),
        message: "This is a commit message".to_string(),
        file_changes: sorted_vector_map!{
            to_mpath("dir1/subdir1/subsubdir1/file_1") => FileChange::tracked(ONES_CTID, FileType::Symlink, 15, None, GitLfs::FullContent),
            to_mpath("dir1/subdir1/subsubdir2/file_1") => FileChange::tracked(TWOS_CTID, FileType::Regular, 17, None, GitLfs::FullContent),
            to_mpath("dir1/subdir1/subsubdir2/file_2") => FileChange::tracked(THREES_CTID, FileType::Regular, 2, None, GitLfs::FullContent),
        },
        ..Default::default()
    }.freeze().expect("Created changeset")
}

async fn run_changeset_hooks(
    ctx: CoreContext,
    bookmark_name: &str,
    hooks: HashMap<String, Box<dyn ChangesetHook>>,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
    expected: HashMap<String, HookExecution>,
) {
    let mut hook_manager = setup_hook_manager(ctx.fb, bookmarks, regexes).await;
    for (hook_name, hook) in hooks {
        hook_manager.register_changeset_hook(&hook_name, hook, Default::default());
    }

    let changeset = default_changeset();
    let res = hook_manager
        .run_changesets_hooks_for_bookmark(
            &ctx,
            &[changeset],
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

async fn run_file_hooks(
    ctx: CoreContext,
    bookmark_name: &str,
    hooks: HashMap<String, Box<dyn FileHook>>,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
    expected: HashMap<String, HashMap<String, HookExecution>>,
) {
    let cs = default_changeset();
    let mut hook_manager = setup_hook_manager(ctx.fb, bookmarks, regexes).await;
    for (hook_name, hook) in hooks {
        hook_manager.register_file_hook(&hook_name, hook, Default::default());
    }
    let res = hook_manager
        .run_changesets_hooks_for_bookmark(
            &ctx,
            &[cs],
            &BookmarkKey::new(bookmark_name).unwrap(),
            None,
            CrossRepoPushSource::NativeToThisRepo,
            PushAuthoredBy::User,
        )
        .await
        .unwrap();
    let map: HashMap<String, HashMap<String, HookExecution>> =
        res.into_iter().fold(HashMap::new(), |mut m, outcome| {
            let path = outcome.get_file_path().expect("Changeset hook").to_string();
            match m.entry(outcome.get_hook_name().to_string()) {
                Entry::Vacant(v) => v.insert(HashMap::new()).insert(path, outcome.into()),
                Entry::Occupied(mut v) => v.get_mut().insert(path, outcome.into()),
            };
            m
        });
    assert_eq!(expected, map);
}

#[mononoke::fbinit_test]
async fn test_changeset_hook_accepted(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let hooks: HashMap<String, Box<dyn ChangesetHook>> = hashmap! {
        "hook1".to_string() => always_accepting_changeset_hook()
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec!["hook1".to_string()]
    };
    let regexes = hashmap! {};
    let expected = hashmap! {
        "hook1".to_string() => HookExecution::Accepted
    };
    run_changeset_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected).await;
}

#[mononoke::fbinit_test]
async fn test_changeset_hook_rejected(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let hooks: HashMap<String, Box<dyn ChangesetHook>> = hashmap! {
        "hook1".to_string() => always_rejecting_changeset_hook()
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec!["hook1".to_string()]
    };
    let regexes = hashmap! {};
    let expected = hashmap! {
        "hook1".to_string() => default_rejection()
    };
    run_changeset_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected).await;
}

#[mononoke::fbinit_test]
async fn test_changeset_hook_mix(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let hooks: HashMap<String, Box<dyn ChangesetHook>> = hashmap! {
        "hook1".to_string() => always_accepting_changeset_hook(),
        "hook2".to_string() => always_rejecting_changeset_hook(),
        "hook3".to_string() => always_accepting_changeset_hook(),
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec!["hook1".to_string(), "hook2".to_string()]
    };
    let regexes = hashmap! {
        "b.*".to_string() => vec!["hook3".to_string()],
    };
    let expected = hashmap! {
        "hook1".to_string() => HookExecution::Accepted,
        "hook2".to_string() => default_rejection(),
        "hook3".to_string() => HookExecution::Accepted,
    };
    run_changeset_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected).await;
}

#[mononoke::fbinit_test]
async fn test_changeset_hook_content_id(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let hook1_map = hashmap! {
        to_mpath("dir1/subdir1/subsubdir1/file_1") => Some(ONES_CTID),
        to_mpath("dir1/subdir1/subsubdir2/file_1") => Some(TWOS_CTID),
        to_mpath("dir1/subdir1/subsubdir2/file_2") => Some(THREES_CTID),
    };
    let hook2_map = hashmap! {
        to_mpath("dir1/subdir1/subsubdir1/file_1") => Some(FOURS_CTID),
        to_mpath("dir1/subdir1/subsubdir2/file_1") => Some(TWOS_CTID),
        to_mpath("dir1/subdir1/subsubdir2/file_2") => Some(THREES_CTID),
    };
    let hook3_map = hashmap! {
        to_mpath("dir1/subdir1/subsubdir1/file_1") => Some(FOURS_CTID),
        to_mpath("dir1/subdir1/subsubdir2/file_1") => Some(FIVES_CTID),
        to_mpath("dir1/subdir1/subsubdir2/file_2") => Some(SIXES_CTID),
    };
    let hooks: HashMap<String, Box<dyn ChangesetHook>> = hashmap! {
        "hook1".to_string() => content_id_matching_changeset_hook(hook1_map),
        "hook2".to_string() => content_id_matching_changeset_hook(hook2_map),
        "hook3".to_string() => content_id_matching_changeset_hook(hook3_map),
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec!["hook1".to_string(), "hook2".to_string()]
    };
    let regexes = hashmap! {
        "b.*".to_string() => vec!["hook2".to_string(), "hook3".to_string()]
    };
    let expected = hashmap! {
        "hook1".to_string() => HookExecution::Accepted,
        "hook2".to_string() => default_rejection(),
        "hook3".to_string() => default_rejection(),
    };
    run_changeset_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected).await;
}

#[mononoke::fbinit_test]
async fn test_file_hook_accepted(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let hooks: HashMap<String, Box<dyn FileHook>> = hashmap! {
        "hook1".to_string() => always_accepting_file_hook()
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec!["hook1".to_string()]
    };
    let regexes = hashmap! {};
    let expected = hashmap! {
        "hook1".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => HookExecution::Accepted,
            "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::Accepted,
            "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::Accepted,
        }
    };
    run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected).await;
}

#[mononoke::fbinit_test]
async fn test_file_hook_rejected(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let hooks: HashMap<String, Box<dyn FileHook>> = hashmap! {
        "hook1".to_string() => always_rejecting_file_hook()
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec!["hook1".to_string()]
    };
    let regexes = hashmap! {};
    let expected = hashmap! {
        "hook1".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => default_rejection(),
        }
    };
    run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected).await;
}

#[mononoke::fbinit_test]
async fn test_file_hook_mix(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let hooks: HashMap<String, Box<dyn FileHook>> = hashmap! {
        "hook1".to_string() => always_rejecting_file_hook(),
        "hook2".to_string() => always_accepting_file_hook()
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec!["hook1".to_string()]
    };
    let regexes = hashmap! {
        "b.*".to_string() => vec!["hook2".to_string()]
    };
    let expected = hashmap! {
        "hook1".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => default_rejection(),
        },
        "hook2".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => HookExecution::Accepted,
            "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::Accepted,
            "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::Accepted,
        }
    };
    run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected).await;
}

#[mononoke::fbinit_test]
async fn test_file_hooks_paths(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let matching_paths = hashset![
        to_mpath("dir1/subdir1/subsubdir2/file_1"),
        to_mpath("dir1/subdir1/subsubdir2/file_2"),
    ];
    let hooks: HashMap<String, Box<dyn FileHook>> = hashmap! {
        "hook1".to_string() => path_matching_file_hook(matching_paths),
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec!["hook1".to_string()]
    };
    let regexes = hashmap! {};
    let expected = hashmap! {
        "hook1".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::Accepted,
            "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::Accepted,
        }
    };
    run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected).await;
}

#[mononoke::fbinit_test]
async fn test_file_hooks_paths_mix(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let matching_paths1 = hashset![
        to_mpath("dir1/subdir1/subsubdir2/file_1"),
        to_mpath("dir1/subdir1/subsubdir2/file_2"),
    ];
    let matching_paths2 = hashset![to_mpath("dir1/subdir1/subsubdir1/file_1"),];
    let hooks: HashMap<String, Box<dyn FileHook>> = hashmap! {
        "hook1".to_string() => path_matching_file_hook(matching_paths1),
        "hook2".to_string() => path_matching_file_hook(matching_paths2),
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec!["hook1".to_string()]
    };
    let regexes = hashmap! {
        "b.*".to_string() => vec!["hook2".to_string()]
    };
    let expected = hashmap! {
        "hook1".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::Accepted,
            "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::Accepted,
        },
        "hook2".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => HookExecution::Accepted,
            "dir1/subdir1/subsubdir2/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => default_rejection(),
        }
    };
    run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected).await;
}

#[mononoke::fbinit_test]
async fn test_file_hook_content_id(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let hooks: HashMap<String, Box<dyn FileHook>> = hashmap! {
        "hook1".to_string() => content_id_matching_file_hook(Some(ONES_CTID)),
        "hook2".to_string() => content_id_matching_file_hook(Some(TWOS_CTID)),
        "hook3".to_string() => content_id_matching_file_hook(Some(THREES_CTID)),
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec!["hook1".to_string(), "hook2".to_string()],
    };
    let regexes = hashmap! {
        "b.*".to_string() => vec!["hook3".to_string()],
    };
    let expected = hashmap! {
        "hook1".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => HookExecution::Accepted,
            "dir1/subdir1/subsubdir2/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => default_rejection(),
        },
        "hook2".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::Accepted,
            "dir1/subdir1/subsubdir2/file_2".to_string() => default_rejection(),
        },
        "hook3".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::Accepted,
        },
    };
    run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected).await;
}

#[mononoke::fbinit_test]
async fn test_file_hook_is_symlink(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let hooks: HashMap<String, Box<dyn FileHook>> = hashmap! {
        "hook1".to_string() => is_symlink_matching_file_hook(true),
        "hook2".to_string() => is_symlink_matching_file_hook(false),
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec!["hook1".to_string()],
    };
    let regexes = hashmap! {
        "b.*".to_string() => vec!["hook2".to_string()],
    };
    let expected = hashmap! {
        "hook1".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => HookExecution::Accepted,
            "dir1/subdir1/subsubdir2/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => default_rejection(),
        },
        "hook2".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::Accepted,
            "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::Accepted,
        },
    };
    run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected).await;
}

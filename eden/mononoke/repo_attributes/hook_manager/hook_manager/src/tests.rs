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
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::Error;
use async_trait::async_trait;
use bookmarks_types::BookmarkKey;
use context::CoreContext;
use fbinit::FacebookInit;
use maplit::hashmap;
use maplit::hashset;
use metaconfig_types::ComparableRegex;
use metaconfig_types::HookBypass;
use metaconfig_types::HookConfig;
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
use permission_checker::AlwaysMember;
use permission_checker::ArcMembershipChecker;
use permission_checker::InternalAclProvider;
use permission_checker::MemberAllowlist;
use permission_checker::MembershipChecker;
use permission_checker::MononokeIdentity;
use permission_checker::MononokeIdentitySet;
use permission_checker::NeverMember;
use repo_permission_checker::NeverAllowRepoPermissionChecker;
use scuba_ext::MononokeScubaSampleBuilder;
use sorted_vector_map::sorted_vector_map;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookExecution;
use crate::HookManager;
use crate::HookOutcome;
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
    let f: fn() -> HookExecution = || HookExecution::accepted();
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

        Ok(HookExecution::accepted())
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
    let f: fn() -> HookExecution = || HookExecution::accepted();
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
            HookExecution::accepted()
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
            Ok(HookExecution::accepted())
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
            HookExecution::accepted()
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
        hook_manager
            .set_hooks_for_bookmark(ComparableRegex::new(&regx).unwrap().into(), hook_names);
    }
    hook_manager
}

fn default_rejection() -> HookExecution {
    HookExecution::rejected(HookRejectionInfo::new_long("desc", "long_desc".to_string()))
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
        hook_manager.register_changeset_hook(&hook_name, hook, Default::default(), None);
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
        hook_manager.register_file_hook(&hook_name, hook, Default::default(), None);
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
        "hook1".to_string() => HookExecution::accepted()
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
        "hook1".to_string() => HookExecution::accepted(),
        "hook2".to_string() => default_rejection(),
        "hook3".to_string() => HookExecution::accepted(),
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
        "hook1".to_string() => HookExecution::accepted(),
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
            "dir1/subdir1/subsubdir1/file_1".to_string() => HookExecution::accepted(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::accepted(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::accepted(),
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
            "dir1/subdir1/subsubdir1/file_1".to_string() => HookExecution::accepted(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::accepted(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::accepted(),
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
            "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::accepted(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::accepted(),
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
            "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::accepted(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::accepted(),
        },
        "hook2".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => HookExecution::accepted(),
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
            "dir1/subdir1/subsubdir1/file_1".to_string() => HookExecution::accepted(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => default_rejection(),
        },
        "hook2".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::accepted(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => default_rejection(),
        },
        "hook3".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::accepted(),
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
            "dir1/subdir1/subsubdir1/file_1".to_string() => HookExecution::accepted(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => default_rejection(),
        },
        "hook2".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => HookExecution::accepted(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => HookExecution::accepted(),
        },
    };
    run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected).await;
}

// =========================================================================
// Bypass permission group tests
// =========================================================================

fn bypass_permission_groups_jk(
    enabled: bool,
    use_client_identities: bool,
) -> justknobs::test_helpers::JustKnobsInMemory {
    justknobs::test_helpers::JustKnobsInMemory::new(
        [
            (
                "scm/mononoke:enable_hook_bypass_permission_groups".to_string(),
                justknobs::test_helpers::KnobVal::Bool(enabled),
            ),
            (
                "scm/mononoke:check_hook_bypass_permission_group_with_client_identities"
                    .to_string(),
                justknobs::test_helpers::KnobVal::Bool(use_client_identities),
            ),
            (
                "scm/mononoke:resolve_bot_fbid_author_for_hook_bypass".to_string(),
                justknobs::test_helpers::KnobVal::Bool(true),
            ),
            (
                "scm/mononoke:resolve_unixname_from_employee_service_for_hook_bypass".to_string(),
                justknobs::test_helpers::KnobVal::Bool(true),
            ),
        ]
        .into(),
    )
}

/// Build a `CoreContext` carrying explicit client identities (as `USER:<id>`).
/// Uses `from_legacy_type_data("USER", id)` to match the existing author tests
/// and avoid a parsing-format dependency.
fn ctx_with_identities(fb: FacebookInit, ids: &[&str]) -> CoreContext {
    let identities: MononokeIdentitySet = ids
        .iter()
        .map(|id| MononokeIdentity::from_legacy_type_data("USER", *id))
        .collect();
    let metadata = metadata::Metadata::default().set_identities(identities);
    let session = context::SessionContainer::builder(fb)
        .metadata(Arc::new(metadata))
        .build();
    CoreContext::test_mock_session(session)
}

fn changeset_with_bypass_msg() -> BonsaiChangeset {
    BonsaiChangesetMut {
        author: "Test User <test@fb.com>".to_string(),
        author_date: DateTime::from_timestamp(1584887580, 0).expect("Getting timestamp"),
        message: "This commit has @bypass_hook in the message".to_string(),
        file_changes: sorted_vector_map! {
            to_mpath("dir1/file.txt") => FileChange::tracked(ONES_CTID, FileType::Regular, 10, None, GitLfs::FullContent),
        },
        ..Default::default()
    }
    .freeze()
    .expect("Created changeset")
}

fn bypass_config_no_group() -> HookConfig {
    HookConfig {
        bypass: Some(HookBypass::new_with_commit_msg("@bypass_hook".to_string())),
        ..Default::default()
    }
}

fn bypass_config_with_group() -> HookConfig {
    HookConfig {
        bypass: Some(
            HookBypass::new_with_commit_msg("@bypass_hook".to_string())
                .with_permission_group(Some("test_bypass_group".to_string())),
        ),
        ..Default::default()
    }
}

fn pushvar_bypass_config_with_group() -> HookConfig {
    HookConfig {
        bypass: Some(
            HookBypass::new_with_pushvar("BYPASS".to_string(), "true".to_string())
                .with_permission_group(Some("test_bypass_group".to_string())),
        ),
        ..Default::default()
    }
}

/// What it tests: with no permission group configured, a bypass string still
/// bypasses the hook (preserves pre-permission-group behavior).
/// Expected: bypassed.
#[mononoke::fbinit_test]
async fn test_bypass_no_group_preserves_behavior(fb: FacebookInit) {
    let res = BypassScenario {
        bypass_config: bypass_config_no_group(),
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_bypassed(&res);
}

/// What it tests: no permission group configured and no bypass string → the
/// hook runs normally (preserves pre-permission-group behavior).
/// Expected: hook runs (rejected).
#[mononoke::fbinit_test]
async fn test_bypass_no_group_no_string_runs_hook(fb: FacebookInit) {
    let res = BypassScenario {
        bypass_config: bypass_config_no_group(),
        changeset: default_changeset(),
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_hook_rejected(&res);
}

/// What it tests: group configured + bypass string + user in group → bypassed.
/// Expected: bypassed.
#[mononoke::fbinit_test]
async fn test_bypass_with_group_authorized_user(fb: FacebookInit) {
    let res = BypassScenario {
        checker: Some(AlwaysMember::new().into()),
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_bypassed(&res);
}

/// What it tests: group configured + bypass string + user NOT in group → the
/// bypass is ignored and the hook runs.
/// Expected: hook runs (rejected).
#[mononoke::fbinit_test]
async fn test_bypass_with_group_unauthorized_user(fb: FacebookInit) {
    let res = BypassScenario {
        checker: Some(NeverMember::new().into()),
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_hook_rejected(&res);
}

/// What it tests: group configured + unauthorized user, but the feature JK is
/// disabled → the bypass falls back to today's (ungated) behavior.
/// Expected: bypassed.
#[mononoke::fbinit_test]
async fn test_bypass_with_group_jk_disabled(fb: FacebookInit) {
    let res = BypassScenario {
        checker: Some(NeverMember::new().into()),
        jk_enabled: false,
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_bypassed(&res);
}

/// What it tests: group configured but no bypass string → the hook runs.
/// Expected: hook runs (rejected).
#[mononoke::fbinit_test]
async fn test_bypass_with_group_no_bypass_string(fb: FacebookInit) {
    let res = BypassScenario {
        checker: Some(NeverMember::new().into()),
        changeset: default_changeset(),
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_hook_rejected(&res);
}

/// What it tests: group configured + pushvar bypass + user in group → bypassed.
/// Expected: bypassed.
#[mononoke::fbinit_test]
async fn test_pushvar_bypass_with_group_authorized(fb: FacebookInit) {
    let res = BypassScenario {
        bypass_config: pushvar_bypass_config_with_group(),
        checker: Some(AlwaysMember::new().into()),
        changeset: default_changeset(),
        pushvars: Some(bypass_pushvars()),
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_bypassed(&res);
}

/// What it tests: group configured + pushvar bypass + user NOT in group → the
/// bypass is ignored and the hook runs.
/// Expected: hook runs (rejected).
#[mononoke::fbinit_test]
async fn test_pushvar_bypass_with_group_unauthorized(fb: FacebookInit) {
    let res = BypassScenario {
        bypass_config: pushvar_bypass_config_with_group(),
        checker: Some(NeverMember::new().into()),
        changeset: default_changeset(),
        pushvars: Some(bypass_pushvars()),
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_hook_rejected(&res);
}

/// What it tests: an unauthorized user attempts a bypass on a commit that the
/// hook would have ACCEPTED. The bypass is moot (nothing to bypass), so the hook
/// runs and the push succeeds rather than being rejected for the bypass attempt.
/// Expected: accepted.
#[mononoke::fbinit_test]
async fn test_unauthorized_bypass_with_accepting_hook(fb: FacebookInit) {
    let res = BypassScenario {
        hook: always_accepting_changeset_hook(),
        checker: Some(NeverMember::new().into()),
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_hook_accepted(&res);
}

/// What it tests: when a hook accepts, the bypass permission group is never
/// consulted even though a bypass token is present -- the membership check is
/// reserved for hooks that actually reject.
/// Expected: accepted, and the membership checker is consulted zero times.
#[mononoke::fbinit_test]
async fn test_accepting_hook_with_bypass_skips_group_check(fb: FacebookInit) {
    let (checker, calls) = CountingMember::new(false);
    let res = BypassScenario {
        hook: always_accepting_changeset_hook(),
        checker: Some(checker),
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_hook_accepted(&res);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "permission group must not be consulted when the hook accepts",
    );
}

/// What it tests: a file hook that rejects several paths of one changeset under an
/// unauthorized bypass checks the permission group exactly once (not once per
/// rejected path) and emits exactly ONE rejection.
#[mononoke::fbinit_test]
async fn test_unauthorized_bypass_file_hook_emits_single_rejection(fb: FacebookInit) {
    let ctx = ctx_with_identities(fb, &[]);
    let (checker, calls) = CountingMember::new(false);
    let mut hook_manager = setup_hook_manager(fb, hashmap! {}, hashmap! {}).await;
    hook_manager.register_file_hook(
        "hook1",
        always_rejecting_file_hook(),
        pushvar_bypass_config_with_group(),
        Some(checker),
    );
    let bm = BookmarkKey::new("bm1").unwrap();
    hook_manager.set_hooks_for_bookmark(bm.clone().into(), vec!["hook1".to_string()]);
    // default_changeset touches 3 files; the file hook rejects each one.
    let changesets = [default_changeset()];
    let res = justknobs::test_helpers::with_just_knobs_async(
        bypass_permission_groups_jk(true, true),
        Box::pin(hook_manager.run_changesets_hooks_for_bookmark(
            &ctx,
            &changesets,
            &bm,
            Some(&bypass_pushvars()),
            CrossRepoPushSource::NativeToThisRepo,
            PushAuthoredBy::User,
        )),
    )
    .await
    .unwrap();
    assert_eq!(
        res.len(),
        1,
        "expected one unauthorized rejection per (hook, changeset), got {res:?}",
    );
    assert!(res[0].get_execution().is_rejected());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "membership must be checked once per (hook, changeset), not once per rejected path",
    );
}

/// What it tests: an unauthorized user attempts a bypass on a commit the hook
/// REJECTS. The hook's own rejection stands, annotated with a note that the
/// bypass was ignored for lack of group membership.
/// Expected: rejected, message carries both the hook reason and the bypass note.
#[mononoke::fbinit_test]
async fn test_unauthorized_bypass_with_rejecting_hook_is_annotated(fb: FacebookInit) {
    let res = BypassScenario {
        checker: Some(NeverMember::new().into()),
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_eq!(res.len(), 1, "expected one outcome, got {res:?}");
    let info = res[0]
        .get_execution()
        .rejection_info()
        .expect("hook rejected");
    // The hook's own reason is preserved...
    assert!(
        info.long_description.contains("long_desc"),
        "expected the hook's own reason, got {:?}",
        info.long_description,
    );
    // ...and the unauthorized-bypass note is appended.
    assert!(
        info.long_description.contains("not a member of group"),
        "expected the bypass note, got {:?}",
        info.long_description,
    );
}

/// What it tests: when no bypass is attempted (no bypass string in the commit),
/// a plain hook rejection is NOT annotated with the bypass note.
/// Expected: rejected, no bypass note.
#[mononoke::fbinit_test]
async fn test_no_bypass_rejection_is_not_annotated(fb: FacebookInit) {
    let res = BypassScenario {
        checker: Some(NeverMember::new().into()),
        changeset: default_changeset(),
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_eq!(res.len(), 1, "expected one outcome, got {res:?}");
    let info = res[0]
        .get_execution()
        .rejection_info()
        .expect("hook rejected");
    assert!(
        !info.long_description.contains("bypass was ignored"),
        "did not expect a bypass note, got {:?}",
        info.long_description,
    );
}

// =========================================================================
// Author-based bypass permission group tests (use_client_identities off)
//
// These verify that group membership is checked against the changeset author's
// identity (USER:<unixname>), not the pusher's TLS cert identity.
// =========================================================================

/// What it tests: on the changeset-author path, the author ("test") is in the
/// allowlist.
/// Expected: bypassed.
#[mononoke::fbinit_test]
async fn test_bypass_checks_commit_author_not_pusher(fb: FacebookInit) {
    let res = BypassScenario {
        checker: Some(allowlist(&["test"])),
        jk_use_client_identities: false,
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_bypassed(&res);
}

/// What it tests: on the changeset-author path, an author ("test") not in the
/// allowlist is denied.
/// Expected: hook runs (rejected).
#[mononoke::fbinit_test]
async fn test_bypass_rejects_when_author_not_in_allowlist(fb: FacebookInit) {
    let res = BypassScenario {
        checker: Some(allowlist(&["someoneelse"])),
        jk_use_client_identities: false,
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_hook_rejected(&res);
}

// =========================================================================
// Client-identity-based bypass permission group tests (use_client_identities on)
//
// These verify that, on the client-identities path, group membership is checked
// against the pusher's client identities, NOT the changeset author.
// =========================================================================

/// What it tests: the client identity ("client_user") is in the allowlist while
/// the author ("test") is NOT, so the bypass is granted via the client identity.
/// Expected: bypassed.
#[mononoke::fbinit_test]
async fn test_bypass_with_client_identities_authorized(fb: FacebookInit) {
    let res = BypassScenario {
        checker: Some(allowlist(&["client_user"])),
        client_identities: vec!["client_user".to_string()],
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_bypassed(&res);
}

/// What it tests: the allowlist contains the author's unixname ("test") but NOT
/// the client identity ("client_user"), so on the client-identities path the
/// author is ignored and the bypass is denied.
/// Expected: hook runs (rejected).
#[mononoke::fbinit_test]
async fn test_bypass_with_client_identities_unauthorized_ignores_author(fb: FacebookInit) {
    let res = BypassScenario {
        checker: Some(allowlist(&["test"])),
        client_identities: vec!["client_user".to_string()],
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_hook_rejected(&res);
}

/// What it tests: on the client-identities path, a request with no client
/// identities fails closed.
/// Expected: hook runs (rejected).
#[mononoke::fbinit_test]
async fn test_bypass_with_empty_client_identities_fails_closed(fb: FacebookInit) {
    let res = BypassScenario {
        checker: Some(allowlist(&[])),
        client_identities: Vec::new(),
        ..Default::default()
    }
    .run(fb)
    .await;
    assert_hook_rejected(&res);
}

// =========================================================================
// Bypass permission group test helpers
// =========================================================================

/// Pushvars that trigger the `BYPASS=true` pushvar bypass.
fn bypass_pushvars() -> HashMap<String, bytes::Bytes> {
    hashmap! { "BYPASS".to_string() => bytes::Bytes::from("true") }
}

/// A permission checker that records how many times it is consulted, so tests can
/// assert the membership check ran the expected number of times.
struct CountingMember {
    is_member: bool,
    calls: Arc<AtomicUsize>,
}

impl CountingMember {
    /// Returns the checker plus a handle to read its call count.
    fn new(is_member: bool) -> (ArcMembershipChecker, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let checker: ArcMembershipChecker = Arc::new(Self {
            is_member,
            calls: calls.clone(),
        });
        (checker, calls)
    }
}

#[async_trait]
impl MembershipChecker for CountingMember {
    async fn is_member(&self, _identities: &MononokeIdentitySet) -> bool {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.is_member
    }
}

/// A `MemberAllowlist` permission checker admitting exactly the given unixnames
/// (as `USER:<unixname>` identities). Pass an empty slice for a checker that
/// admits no one (used to exercise the fail-closed path).
fn allowlist(unixnames: &[&str]) -> ArcMembershipChecker {
    let identities: MononokeIdentitySet = unixnames
        .iter()
        .map(|name| MononokeIdentity::from_legacy_type_data("USER", *name))
        .collect();
    MemberAllowlist::new(identities).into()
}

/// Assert the bypass was honored: the always-rejecting hook did not run.
fn assert_bypassed(outcomes: &[HookOutcome]) {
    assert!(
        outcomes.is_empty(),
        "expected the bypass to be honored (hook skipped), got: {outcomes:?}",
    );
}

/// Assert the hook ran and accepted (e.g. an unauthorized bypass was ignored but
/// the hook had nothing to object to).
fn assert_hook_accepted(outcomes: &[HookOutcome]) {
    assert_eq!(
        outcomes.len(),
        1,
        "expected the hook to run, got {} outcome(s)",
        outcomes.len(),
    );
    assert!(
        outcomes[0].get_execution().is_accepted(),
        "expected the hook to accept, got {:?}",
        outcomes[0].get_execution(),
    );
}

/// Assert the bypass was NOT honored: the always-rejecting hook ran and rejected.
fn assert_hook_rejected(outcomes: &[HookOutcome]) {
    assert_eq!(
        outcomes.len(),
        1,
        "expected the hook to run, got {} outcome(s)",
        outcomes.len(),
    );
    assert!(
        outcomes[0].get_execution().is_rejected(),
        "expected the hook to reject, got {:?}",
        outcomes[0].get_execution(),
    );
}

/// A single bypass-permission-group scenario. Registers one changeset hook
/// ("hook1", default: always-rejecting) on bookmark "bm1" with `bypass_config` +
/// `checker`, then runs it over `changeset` under the given JustKnobs and client
/// identities, and returns the hook outcomes.
///
/// Construct it with struct-update syntax so each test overrides only the
/// fields relevant to its scenario, e.g.
/// `BypassScenario { checker: Some(AlwaysMember::new().into()), ..Default::default() }`.
///
/// Defaults: a permission-group bypass config, an always-rejecting hook, no
/// checker, a changeset carrying the bypass message, no pushvars, no client
/// identities, the feature enabled, and the client-identities path selected.
struct BypassScenario {
    bypass_config: HookConfig,
    hook: Box<dyn ChangesetHook>,
    checker: Option<ArcMembershipChecker>,
    changeset: BonsaiChangeset,
    pushvars: Option<HashMap<String, bytes::Bytes>>,
    client_identities: Vec<String>,
    jk_enabled: bool,
    jk_use_client_identities: bool,
}

impl Default for BypassScenario {
    fn default() -> Self {
        Self {
            bypass_config: bypass_config_with_group(),
            hook: always_rejecting_changeset_hook(),
            checker: None,
            changeset: changeset_with_bypass_msg(),
            pushvars: None,
            client_identities: Vec::new(),
            jk_enabled: true,
            jk_use_client_identities: true,
        }
    }
}

impl BypassScenario {
    async fn run(self, fb: FacebookInit) -> Vec<HookOutcome> {
        let ctx = {
            let id_refs: Vec<&str> = self.client_identities.iter().map(String::as_str).collect();
            ctx_with_identities(fb, &id_refs)
        };

        let mut hook_manager = setup_hook_manager(fb, hashmap! {}, hashmap! {}).await;
        hook_manager.register_changeset_hook("hook1", self.hook, self.bypass_config, self.checker);
        let bm = BookmarkKey::new("bm1").unwrap();
        hook_manager.set_hooks_for_bookmark(bm.clone().into(), vec!["hook1".to_string()]);

        let changesets = [self.changeset];
        justknobs::test_helpers::with_just_knobs_async(
            bypass_permission_groups_jk(self.jk_enabled, self.jk_use_client_identities),
            Box::pin(hook_manager.run_changesets_hooks_for_bookmark(
                &ctx,
                &changesets,
                &bm,
                self.pushvars.as_ref(),
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )),
        )
        .await
        .unwrap()
    }
}

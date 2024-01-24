/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::Error;
use async_trait::async_trait;
use bookmarks_types::BookmarkKey;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future;
use futures::stream::futures_unordered;
use futures::stream::TryStreamExt;
use maplit::hashmap;
use maplit::hashset;
use metaconfig_types::HookManagerParams;
use mononoke_types::BasicFileChange;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use mononoke_types_mocks::contentid::ONES_CTID;
use mononoke_types_mocks::contentid::THREES_CTID;
use mononoke_types_mocks::contentid::TWOS_CTID;
use permission_checker::InternalAclProvider;
use regex::Regex;
use scuba_ext::MononokeScubaSampleBuilder;
use sorted_vector_map::sorted_vector_map;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookExecution;
use crate::HookFileContentProvider;
use crate::HookManager;
use crate::HookRejectionInfo;
use crate::InMemoryHookFileContentProvider;
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
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _bookmark: &BookmarkKey,
        _changeset: &'cs BonsaiChangeset,
        _content_manager: &'fetcher dyn HookFileContentProvider,
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
struct FileContentMatchingChangesetHook {
    expected_content: HashMap<NonRootMPath, Option<String>>,
}

#[async_trait]
impl ChangesetHook for FileContentMatchingChangesetHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        content_manager: &'fetcher dyn HookFileContentProvider,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        let futs = futures_unordered::FuturesUnordered::new();

        for (path, change) in changeset.simplified_file_changes() {
            // If we have a change to a path, but no expected change, fail
            let expected_content = match self.expected_content.get(path) {
                Some(expected) => expected,
                None => return Ok(default_rejection()),
            };

            match change {
                Some(change) => {
                    let fut = async move {
                        let content = content_manager
                            .get_file_text(ctx, change.content_id())
                            .await?;
                        let content =
                            content.map(|c| std::str::from_utf8(c.as_ref()).unwrap().to_string());

                        // True only if there is content containing the expected content
                        Ok(match (content, expected_content.as_ref()) {
                            (Some(content), Some(expected_content)) => {
                                content.contains(expected_content)
                            }
                            (None, None) => true,
                            _ => false,
                        })
                    };
                    futs.push(fut);
                }
                None => {
                    // If we have a deletion, but expect it to be present, fail
                    if expected_content.is_some() {
                        return Ok(default_rejection());
                    }
                }
            }
        }

        let opt_item = futs
            .try_skip_while(|b: &bool| future::ok::<_, Error>(*b))
            .try_next()
            .await?;
        Ok(if opt_item.is_some() {
            default_rejection()
        } else {
            HookExecution::Accepted
        })
    }
}

fn file_text_matching_changeset_hook(
    expected_content: HashMap<NonRootMPath, Option<String>>,
) -> Box<dyn ChangesetHook> {
    Box::new(FileContentMatchingChangesetHook { expected_content })
}

#[derive(Clone, Debug)]
struct LengthMatchingChangesetHook {
    expected_lengths: HashMap<NonRootMPath, u64>,
}

#[async_trait]
impl ChangesetHook for LengthMatchingChangesetHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        content_manager: &'fetcher dyn HookFileContentProvider,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        let futs = futures_unordered::FuturesUnordered::new();
        for (path, change) in changeset.simplified_file_changes() {
            let expected_length = self.expected_lengths.get(path);

            match change {
                Some(change) => {
                    let fut = async move {
                        let size = content_manager
                            .get_file_size(ctx, change.content_id())
                            .await?;

                        Ok(expected_length == Some(size).as_ref())
                    };
                    futs.push(fut);
                }
                None => {
                    if expected_length.is_some() {
                        return Ok(default_rejection());
                    }
                }
            }
        }
        let opt_item = futs
            .try_skip_while(|b: &bool| future::ok::<_, Error>(*b))
            .try_next()
            .await?;
        Ok(if opt_item.is_some() {
            default_rejection()
        } else {
            HookExecution::Accepted
        })
    }
}

fn length_matching_changeset_hook(
    expected_lengths: HashMap<NonRootMPath, u64>,
) -> Box<dyn ChangesetHook> {
    Box::new(LengthMatchingChangesetHook { expected_lengths })
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
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _content_manager: &'fetcher dyn HookFileContentProvider,
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
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _content_manager: &'fetcher dyn HookFileContentProvider,
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
struct FileContentMatchingFileHook {
    expected_content: Option<String>,
}

#[async_trait]
impl FileHook for FileContentMatchingFileHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        content_manager: &'fetcher dyn HookFileContentProvider,
        change: Option<&'change BasicFileChange>,
        _path: &'path NonRootMPath,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        match change {
            Some(change) => {
                let content = content_manager
                    .get_file_text(ctx, change.content_id())
                    .await?;
                let content = content.map(|c| std::str::from_utf8(c.as_ref()).unwrap().to_string());
                Ok(match (content, self.expected_content.as_ref()) {
                    (Some(content), Some(expected_content)) => {
                        if content.contains(expected_content) {
                            HookExecution::Accepted
                        } else {
                            default_rejection()
                        }
                    }
                    (None, None) => HookExecution::Accepted,
                    _ => default_rejection(),
                })
            }

            None => Ok(default_rejection()),
        }
    }
}

fn file_text_matching_file_hook(expected_content: Option<String>) -> Box<dyn FileHook> {
    Box::new(FileContentMatchingFileHook { expected_content })
}

#[derive(Clone, Debug)]
struct IsSymLinkMatchingFileHook {
    is_symlink: bool,
}

#[async_trait]
impl FileHook for IsSymLinkMatchingFileHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _content_manager: &'fetcher dyn HookFileContentProvider,
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

#[derive(Clone, Debug)]
struct LengthMatchingFileHook {
    length: u64,
}

#[async_trait]
impl FileHook for LengthMatchingFileHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        content_manager: &'fetcher dyn HookFileContentProvider,
        change: Option<&'change BasicFileChange>,
        _path: &'path NonRootMPath,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        let length = match change {
            Some(change) => {
                content_manager
                    .get_file_size(ctx, change.content_id())
                    .await?
            }
            None => return Ok(HookExecution::Accepted),
        };
        if length == self.length {
            return Ok(HookExecution::Accepted);
        }
        Ok(default_rejection())
    }
}

fn length_matching_file_hook(length: u64) -> Box<dyn FileHook> {
    Box::new(LengthMatchingFileHook { length })
}

async fn hook_manager_inmem(fb: FacebookInit) -> HookManager {
    let ctx = CoreContext::test_mock(fb);

    let mut content_manager = InMemoryHookFileContentProvider::new();
    content_manager.insert(ONES_CTID, "elephants");
    content_manager.insert(TWOS_CTID, "hippopatami");
    content_manager.insert(THREES_CTID, "eels");

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

async fn setup_hook_manager(
    fb: FacebookInit,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
) -> HookManager {
    let mut hook_manager = hook_manager_inmem(fb).await;
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
        .run_hooks_for_bookmark(
            &ctx,
            vec![cs].iter(),
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

#[fbinit::test]
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

#[fbinit::test]
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

#[fbinit::test]
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

#[fbinit::test]
async fn test_changeset_hook_file_text(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let hook1_map = hashmap![
        to_mpath("dir1/subdir1/subsubdir1/file_1") => Some("elephants".to_string()),
        to_mpath("dir1/subdir1/subsubdir2/file_1") => Some("hippopatami".to_string()),
        to_mpath("dir1/subdir1/subsubdir2/file_2") => Some("eels".to_string()),
    ];
    let hook2_map = hashmap![
        to_mpath("dir1/subdir1/subsubdir1/file_1") => Some("anteaters".to_string()),
        to_mpath("dir1/subdir1/subsubdir2/file_1") => Some("hippopatami".to_string()),
        to_mpath("dir1/subdir1/subsubdir2/file_2") => Some("eels".to_string()),
    ];
    let hook3_map = hashmap![
        to_mpath("dir1/subdir1/subsubdir1/file_1") => Some("anteaters".to_string()),
        to_mpath("dir1/subdir1/subsubdir2/file_1") => Some("giraffes".to_string()),
        to_mpath("dir1/subdir1/subsubdir2/file_2") => Some("lions".to_string()),
    ];
    let hooks: HashMap<String, Box<dyn ChangesetHook>> = hashmap! {
        "hook1".to_string() => file_text_matching_changeset_hook(hook1_map),
        "hook2".to_string() => file_text_matching_changeset_hook(hook2_map),
        "hook3".to_string() => file_text_matching_changeset_hook(hook3_map),
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

#[fbinit::test]
async fn test_changeset_hook_lengths(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let hook1_map = hashmap![
        to_mpath("dir1/subdir1/subsubdir1/file_1") => 9,
        to_mpath("dir1/subdir1/subsubdir2/file_1") => 11,
        to_mpath("dir1/subdir1/subsubdir2/file_2") => 4
    ];
    let hook2_map = hashmap![
        to_mpath("dir1/subdir1/subsubdir1/file_1") => 9,
        to_mpath("dir1/subdir1/subsubdir2/file_1") => 12,
        to_mpath("dir1/subdir1/subsubdir2/file_2") => 4
    ];
    let hook3_map = hashmap![
        to_mpath("dir1/subdir1/subsubdir1/file_1") => 15,
        to_mpath("dir1/subdir1/subsubdir2/file_1") => 17,
        to_mpath("dir1/subdir1/subsubdir2/file_2") => 2
    ];
    let hooks: HashMap<String, Box<dyn ChangesetHook>> = hashmap! {
        "hook1".to_string() => length_matching_changeset_hook(hook1_map),
        "hook2".to_string() => length_matching_changeset_hook(hook2_map),
        "hook3".to_string() => length_matching_changeset_hook(hook3_map),
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec!["hook1".to_string(), "hook2".to_string()],
    };
    let regexes = hashmap! {
        "b.*".to_string() => vec!["hook3".to_string()],
    };
    let expected = hashmap! {
        "hook1".to_string() => HookExecution::Accepted,
        "hook2".to_string() => default_rejection(),
        "hook3".to_string() => default_rejection(),
    };
    run_changeset_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected).await;
}

#[fbinit::test]
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

#[fbinit::test]
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

#[fbinit::test]
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

#[fbinit::test]
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

#[fbinit::test]
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

#[fbinit::test]
async fn test_file_hook_file_text(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let hooks: HashMap<String, Box<dyn FileHook>> = hashmap! {
        "hook1".to_string() => file_text_matching_file_hook(Some("elephants".to_string())),
        "hook2".to_string() => file_text_matching_file_hook(Some("hippopatami".to_string())),
        "hook3".to_string() => file_text_matching_file_hook(Some("eels".to_string()))
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

#[fbinit::test]
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

#[fbinit::test]
async fn test_file_hook_length(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let hooks: HashMap<String, Box<dyn FileHook>> = hashmap! {
        "hook1".to_string() => length_matching_file_hook("elephants".len() as u64),
        "hook2".to_string() => length_matching_file_hook("hippopatami".len() as u64),
        "hook3".to_string() => length_matching_file_hook("eels".len() as u64),
        "hook4".to_string() => length_matching_file_hook(999)
    };
    let bookmarks = hashmap! {
        "bm1".to_string() => vec!["hook1".to_string(), "hook2".to_string(), "hook3".to_string()],
    };
    let regexes = hashmap! {
        "b.*".to_string() => vec!["hook3".to_string(), "hook4".to_string()],
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
        "hook4".to_string() => hashmap! {
            "dir1/subdir1/subsubdir1/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => default_rejection(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => default_rejection(),
        },
    };
    run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected).await;
}

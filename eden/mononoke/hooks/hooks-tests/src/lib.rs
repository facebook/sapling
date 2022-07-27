/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use context::CoreContext;
use fbinit::FacebookInit;
use fixtures::TestRepoFixture;
use futures::future;
use futures::stream::futures_unordered;
use futures::stream::TryStreamExt;
use futures::TryFutureExt;
use hooks::hook_loader::load_hooks;
use hooks::ChangesetHook;
use hooks::CrossRepoPushSource;
use hooks::ErrorKind;
use hooks::FileHook;
use hooks::HookExecution;
use hooks::HookManager;
use hooks::HookRejectionInfo;
use hooks::PushAuthoredBy;
use hooks_content_stores::FileChange as FileDiff;
use hooks_content_stores::FileContentManager;
use hooks_content_stores::InMemoryFileContentManager;
use hooks_content_stores::PathContent;
use hooks_content_stores::RepoFileContentManager;
use maplit::btreemap;
use maplit::hashmap;
use maplit::hashset;
use metaconfig_types::BookmarkParams;
use metaconfig_types::HookConfig;
use metaconfig_types::HookManagerParams;
use metaconfig_types::HookParams;
use metaconfig_types::RepoConfig;
use mononoke_types::BasicFileChange;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types_mocks::contentid::ONES_CTID;
use mononoke_types_mocks::contentid::THREES_CTID;
use mononoke_types_mocks::contentid::TWOS_CTID;
use permission_checker::DefaultAclProvider;
use regex::Regex;
use repo_blobstore::RepoBlobstoreRef;
use scuba_ext::MononokeScubaSampleBuilder;
use sorted_vector_map::sorted_vector_map;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::collections::HashSet;
use tests_utils::bookmark;
use tests_utils::create_commit;
use tests_utils::store_files;
use tests_utils::BasicTestRepo;
use tests_utils::CreateCommitContext;

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
        _bookmark: &BookmarkName,
        _changeset: &'cs BonsaiChangeset,
        _content_manager: &'fetcher dyn FileContentManager,
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

#[derive(Clone)]
struct FindFilesChangesetHook {
    pub filename: String,
}

#[async_trait]
impl ChangesetHook for FindFilesChangesetHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        _bookmark: &BookmarkName,
        _changeset: &'cs BonsaiChangeset,
        content_manager: &'fetcher dyn FileContentManager,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        let path = to_mpath(self.filename.as_str());
        let res = content_manager
            .find_content(ctx, BookmarkName::new("master")?, vec![path.clone()])
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
                Err(err).map_err(Error::from)
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
        _bookmark: &BookmarkName,
        changeset: &'cs BonsaiChangeset,
        content_manager: &'fetcher dyn FileContentManager,
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

#[derive(Clone, Debug)]
struct FileContentMatchingChangesetHook {
    expected_content: HashMap<MPath, Option<String>>,
}

#[derive(Clone)]
struct LatestChangesChangesetHook(HashMap<MPath, Option<ChangesetId>>);

#[async_trait]
impl ChangesetHook for LatestChangesChangesetHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        _bookmark: &BookmarkName,
        _changeset: &'cs BonsaiChangeset,
        content_manager: &'fetcher dyn FileContentManager,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        let paths = self.0.keys().cloned().collect();
        let res = content_manager
            .latest_changes(ctx, BookmarkName::new("master")?, paths)
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

#[async_trait]
impl ChangesetHook for FileContentMatchingChangesetHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        _bookmark: &BookmarkName,
        changeset: &'cs BonsaiChangeset,
        content_manager: &'fetcher dyn FileContentManager,
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
    expected_content: HashMap<MPath, Option<String>>,
) -> Box<dyn ChangesetHook> {
    Box::new(FileContentMatchingChangesetHook { expected_content })
}

#[derive(Clone, Debug)]
struct LengthMatchingChangesetHook {
    expected_lengths: HashMap<MPath, u64>,
}

#[async_trait]
impl ChangesetHook for LengthMatchingChangesetHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        _bookmark: &BookmarkName,
        changeset: &'cs BonsaiChangeset,
        content_manager: &'fetcher dyn FileContentManager,
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

fn length_matching_changeset_hook(expected_lengths: HashMap<MPath, u64>) -> Box<dyn ChangesetHook> {
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
        _content_manager: &'fetcher dyn FileContentManager,
        _change: Option<&'change BasicFileChange>,
        _path: &'path MPath,
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
    paths: HashSet<MPath>,
}

#[async_trait]
impl FileHook for PathMatchingFileHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _content_manager: &'fetcher dyn FileContentManager,
        _change: Option<&'change BasicFileChange>,
        path: &'path MPath,
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

fn path_matching_file_hook(paths: HashSet<MPath>) -> Box<dyn FileHook> {
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
        content_manager: &'fetcher dyn FileContentManager,
        change: Option<&'change BasicFileChange>,
        _path: &'path MPath,
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
        _content_manager: &'fetcher dyn FileContentManager,
        change: Option<&'change BasicFileChange>,
        _path: &'path MPath,
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
        content_manager: &'fetcher dyn FileContentManager,
        change: Option<&'change BasicFileChange>,
        _path: &'path MPath,
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
    run_file_hooks(
        ctx,
        "bm1",
        hooks,
        bookmarks,
        regexes,
        expected,
        ContentFetcherType::InMemory,
    )
    .await;
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
    run_file_hooks(
        ctx,
        "bm1",
        hooks,
        bookmarks,
        regexes,
        expected,
        ContentFetcherType::InMemory,
    )
    .await;
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
    run_file_hooks(
        ctx,
        "bm1",
        hooks,
        bookmarks,
        regexes,
        expected,
        ContentFetcherType::InMemory,
    )
    .await;
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
    run_file_hooks(
        ctx,
        "bm1",
        hooks,
        bookmarks,
        regexes,
        expected,
        ContentFetcherType::InMemory,
    )
    .await;
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
    run_file_hooks(
        ctx,
        "bm1",
        hooks,
        bookmarks,
        regexes,
        expected,
        ContentFetcherType::InMemory,
    )
    .await;
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
    run_file_hooks(
        ctx,
        "bm1",
        hooks,
        bookmarks,
        regexes,
        expected,
        ContentFetcherType::InMemory,
    )
    .await;
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
    run_file_hooks(
        ctx,
        "bm1",
        hooks,
        bookmarks,
        regexes,
        expected,
        ContentFetcherType::InMemory,
    )
    .await;
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
    run_file_hooks(
        ctx,
        "bm1",
        hooks,
        bookmarks,
        regexes,
        expected,
        ContentFetcherType::InMemory,
    )
    .await;
}

#[fbinit::test]
async fn test_cs_find_content_hook_with_blob_store(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BasicTestRepo = test_repo_factory::build_empty(fb)?;
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
        None,
        "bm1",
        hooks,
        bookmarks,
        regexes.clone(),
        expected,
        ContentFetcherType::Blob(repo.clone()),
    )
    .await;

    // set master bookmark
    let mut txn = repo.bookmarks().create_transaction(ctx.clone());
    txn.force_set(
        &BookmarkName::new("master")?,
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
        ctx.clone(),
        None,
        "bm1",
        hooks,
        bookmarks,
        regexes.clone(),
        expected,
        ContentFetcherType::Blob(repo),
    )
    .await;

    Ok(())
}

#[fbinit::test]
async fn test_cs_file_changes_hook_with_blob_store(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BasicTestRepo = test_repo_factory::build_empty(fb)?;
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
        ctx.clone(),
        Some(changeset),
        "bm1",
        hooks,
        bookmarks,
        regexes.clone(),
        expected,
        ContentFetcherType::Blob(repo),
    )
    .await;

    Ok(())
}

#[fbinit::test]
async fn test_cs_latest_changes_hook_with_blob_store(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BasicTestRepo = test_repo_factory::build_empty(fb)?;
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
        ctx.clone(),
        None,
        "bm1",
        hooks,
        bookmarks,
        regexes.clone(),
        expected,
        ContentFetcherType::Blob(repo),
    )
    .await;

    Ok(())
}

#[fbinit::test]
async fn test_cs_hooks_with_blob_store(fb: FacebookInit) {
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
    run_changeset_hooks_with_mgr(
        ctx.clone(),
        None,
        "bm1",
        hooks,
        bookmarks,
        regexes.clone(),
        expected,
        ContentFetcherType::Blob(fixtures::ManyFilesDirs::get_test_repo(ctx.fb).await),
    )
    .await;
}

#[fbinit::test]
async fn test_file_hooks_with_blob_store(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    // Create an init a repo
    let (repo, bcs_id) = {
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb).unwrap();

        let parent = create_commit(
            ctx.clone(),
            repo.clone(),
            vec![],
            store_files(&ctx, btreemap! {"toremove" => Some("content")}, &repo).await,
        )
        .await;
        let bcs_id = create_commit(
            ctx.clone(),
            repo.clone(),
            vec![parent],
            store_files(
                &ctx,
                btreemap! {
                    "toremove" => None,
                    "newfile" => Some("newcontent"),
                    "dir/somefile" => Some("good"),
                },
                &repo,
            )
            .await,
        )
        .await;

        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        txn.force_set(
            &BookmarkName::new("master").unwrap(),
            bcs_id,
            BookmarkUpdateReason::TestMove,
        )
        .unwrap();
        txn.commit().await.unwrap();
        (repo, bcs_id)
    };

    let bookmarks = hashmap! {
        "master".to_string() => vec!["hook1".to_string()]
    };
    let hooks: HashMap<String, Box<dyn FileHook>> = hashmap! {
        "hook1".to_string() => length_matching_file_hook(4),
    };
    let regexes = hashmap! {};

    let expected = hashmap! {
        "hook1".to_string() => hashmap! {
            "newfile".to_string() => default_rejection(),
            "dir/somefile".to_string() => HookExecution::Accepted,
            "toremove".to_string() => HookExecution::Accepted,
        },
    };

    let bcs = bcs_id
        .load(&ctx, repo.repo_blobstore())
        .await
        .expect("Can't load commit");
    run_file_hooks_for_cs(
        ctx,
        "master",
        hooks,
        bookmarks,
        regexes,
        expected,
        ContentFetcherType::Blob(repo),
        bcs,
    )
    .await;
}

async fn run_changeset_hooks(
    ctx: CoreContext,
    bookmark_name: &str,
    hooks: HashMap<String, Box<dyn ChangesetHook>>,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
    expected: HashMap<String, HookExecution>,
) {
    run_changeset_hooks_with_mgr(
        ctx,
        None,
        bookmark_name,
        hooks,
        bookmarks,
        regexes,
        expected,
        ContentFetcherType::InMemory,
    )
    .await
}

async fn run_changeset_hooks_with_mgr(
    ctx: CoreContext,
    changeset: Option<BonsaiChangeset>,
    bookmark_name: &str,
    hooks: HashMap<String, Box<dyn ChangesetHook>>,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
    expected: HashMap<String, HookExecution>,
    content_manager_type: ContentFetcherType,
) {
    let mut hook_manager =
        setup_hook_manager(ctx.fb, bookmarks, regexes, content_manager_type).await;
    for (hook_name, hook) in hooks {
        hook_manager.register_changeset_hook(&hook_name, hook, Default::default());
    }

    let changeset = changeset.unwrap_or_else(default_changeset);
    let res = hook_manager
        .run_hooks_for_bookmark(
            &ctx,
            vec![changeset].iter(),
            &BookmarkName::new(bookmark_name).unwrap(),
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

enum ContentFetcherType {
    InMemory,
    Blob(BasicTestRepo),
}

async fn run_file_hooks(
    ctx: CoreContext,
    bookmark_name: &str,
    hooks: HashMap<String, Box<dyn FileHook>>,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
    expected: HashMap<String, HashMap<String, HookExecution>>,
    content_manager_type: ContentFetcherType,
) {
    run_file_hooks_with_mgr(
        ctx,
        bookmark_name,
        hooks,
        bookmarks,
        regexes,
        expected,
        content_manager_type,
        default_changeset(),
    )
    .await
}

async fn run_file_hooks_for_cs(
    ctx: CoreContext,
    bookmark_name: &str,
    hooks: HashMap<String, Box<dyn FileHook>>,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
    expected: HashMap<String, HashMap<String, HookExecution>>,
    content_manager_type: ContentFetcherType,
    cs: BonsaiChangeset,
) {
    run_file_hooks_with_mgr(
        ctx,
        bookmark_name,
        hooks,
        bookmarks,
        regexes,
        expected,
        content_manager_type,
        cs,
    )
    .await
}

async fn run_file_hooks_with_mgr(
    ctx: CoreContext,
    bookmark_name: &str,
    hooks: HashMap<String, Box<dyn FileHook>>,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
    expected: HashMap<String, HashMap<String, HookExecution>>,
    content_manager_type: ContentFetcherType,
    cs: BonsaiChangeset,
) {
    let mut hook_manager =
        setup_hook_manager(ctx.fb, bookmarks, regexes, content_manager_type).await;
    for (hook_name, hook) in hooks {
        hook_manager.register_file_hook(&hook_name, hook, Default::default());
    }
    let res = hook_manager
        .run_hooks_for_bookmark(
            &ctx,
            vec![cs].iter(),
            &BookmarkName::new(bookmark_name).unwrap(),
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

async fn setup_hook_manager(
    fb: FacebookInit,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
    content_manager_type: ContentFetcherType,
) -> HookManager {
    let mut hook_manager = match content_manager_type {
        ContentFetcherType::InMemory => hook_manager_inmem(fb).await,
        ContentFetcherType::Blob(repo) => hook_manager_repo(fb, &repo).await,
    };
    for (bookmark_name, hook_names) in bookmarks {
        hook_manager
            .set_hooks_for_bookmark(BookmarkName::new(bookmark_name).unwrap().into(), hook_names);
    }
    for (regx, hook_names) in regexes {
        hook_manager.set_hooks_for_bookmark(Regex::new(&regx).unwrap().into(), hook_names);
    }
    hook_manager
}

fn default_rejection() -> HookExecution {
    HookExecution::Rejected(HookRejectionInfo::new_long("desc", "long_desc".to_string()))
}

fn default_changeset() -> BonsaiChangeset {
    BonsaiChangesetMut {
        parents: Vec::new(),
        author: "Jeremy Fitzhardinge <jsgf@fb.com>".to_string(),
        author_date: DateTime::from_timestamp(1584887580, 0).expect("Getting timestamp"),
        committer: None,
        committer_date: None,
        message: "This is a commit message".to_string(),
        extra: Default::default(),
        file_changes: sorted_vector_map!{
            to_mpath("dir1/subdir1/subsubdir1/file_1") => FileChange::tracked(ONES_CTID, FileType::Symlink, 15, None),
            to_mpath("dir1/subdir1/subsubdir2/file_1") => FileChange::tracked(TWOS_CTID, FileType::Regular, 17, None),
            to_mpath("dir1/subdir1/subsubdir2/file_2") => FileChange::tracked(THREES_CTID, FileType::Regular, 2, None),
        },
        is_snapshot: false,
    }.freeze().expect("Created changeset")
}

async fn hook_manager_repo(fb: FacebookInit, repo: &BasicTestRepo) -> HookManager {
    let ctx = CoreContext::test_mock(fb);

    let content_manager = RepoFileContentManager::new(&repo);
    HookManager::new(
        ctx.fb,
        DefaultAclProvider::new(fb),
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

async fn hook_manager_many_files_dirs_repo(fb: FacebookInit) -> HookManager {
    hook_manager_repo(fb, &fixtures::ManyFilesDirs::get_test_repo(fb).await).await
}

fn to_mpath(string: &str) -> MPath {
    MPath::new(string).unwrap()
}

async fn hook_manager_inmem(fb: FacebookInit) -> HookManager {
    let ctx = CoreContext::test_mock(fb);

    let mut content_manager = InMemoryFileContentManager::new();
    content_manager.insert(ONES_CTID, "elephants");
    content_manager.insert(TWOS_CTID, "hippopatami");
    content_manager.insert(THREES_CTID, "eels");

    HookManager::new(
        ctx.fb,
        DefaultAclProvider::new(fb),
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

#[fbinit::test]
async fn test_verify_integrity_fast_failure(fb: FacebookInit) {
    let mut config = RepoConfig::default();
    config.bookmarks = vec![BookmarkParams {
        bookmark: Regex::new("bm2").unwrap().into(),
        hooks: vec!["verify_integrity".into()],
        only_fast_forward: false,
        allowed_users: None,
        allowed_hipster_group: None,
        rewrite_dates: None,
        hooks_skip_ancestors_of: vec![],
        ensure_ancestor_of: None,
        allow_move_to_public_commits_without_hooks: false,
    }];
    config.hooks = vec![HookParams {
        name: "verify_integrity".into(),
        config: HookConfig {
            strings: hashmap! {String::from("verify_integrity_path") => String::from("bad_nonexisting_filename")},
            ..Default::default()
        },
    }];

    let mut hm = hook_manager_many_files_dirs_repo(fb).await;
    load_hooks(
        fb,
        &DefaultAclProvider::new(fb),
        &mut hm,
        &config,
        &hashset![],
    )
    .await
    .expect_err("`verify_integrity` hook loading should have failed");
}

#[fbinit::test]
async fn test_load_hooks_bad_rust_hook(fb: FacebookInit) {
    let mut config = RepoConfig::default();
    config.bookmarks = vec![BookmarkParams {
        bookmark: BookmarkName::new("bm1").unwrap().into(),
        hooks: vec!["hook1".into()],
        only_fast_forward: false,
        allowed_users: None,
        allowed_hipster_group: None,
        rewrite_dates: None,
        hooks_skip_ancestors_of: vec![],
        ensure_ancestor_of: None,
        allow_move_to_public_commits_without_hooks: false,
    }];

    config.hooks = vec![HookParams {
        name: "hook1".into(),
        config: Default::default(),
    }];

    let mut hm = hook_manager_many_files_dirs_repo(fb).await;

    match load_hooks(
        fb,
        &DefaultAclProvider::new(fb),
        &mut hm,
        &config,
        &hashset![],
    )
    .await
    .unwrap_err()
    .downcast::<ErrorKind>()
    {
        Ok(ErrorKind::InvalidRustHook(hook_name)) => {
            assert_eq!(hook_name, "hook1".to_string());
        }
        _ => assert!(false, "Unexpected err type"),
    };
}

#[fbinit::test]
async fn test_load_disabled_hooks(fb: FacebookInit) {
    let mut config = RepoConfig::default();

    config.bookmarks = vec![];

    config.hooks = vec![HookParams {
        name: "hook1".into(),
        config: Default::default(),
    }];

    let mut hm = hook_manager_many_files_dirs_repo(fb).await;

    load_hooks(
        fb,
        &DefaultAclProvider::new(fb),
        &mut hm,
        &config,
        &hashset!["hook1".to_string()],
    )
    .await
    .expect("disabling a broken hook should allow loading to succeed");
}

#[fbinit::test]
async fn test_load_disabled_hooks_referenced_by_bookmark(fb: FacebookInit) {
    let mut config = RepoConfig::default();

    config.bookmarks = vec![BookmarkParams {
        bookmark: BookmarkName::new("bm1").unwrap().into(),
        hooks: vec!["hook1".into()],
        only_fast_forward: false,
        allowed_users: None,
        allowed_hipster_group: None,
        rewrite_dates: None,
        hooks_skip_ancestors_of: vec![],
        ensure_ancestor_of: None,
        allow_move_to_public_commits_without_hooks: false,
    }];

    config.hooks = vec![HookParams {
        name: "hook1".into(),
        config: Default::default(),
    }];

    let mut hm = hook_manager_many_files_dirs_repo(fb).await;

    load_hooks(
        fb,
        &DefaultAclProvider::new(fb),
        &mut hm,
        &config,
        &hashset!["hook1".to_string()],
    )
    .await
    .expect("disabling a broken hook should allow loading to succeed");
}

#[fbinit::test]
async fn test_load_disabled_hooks_hook_does_not_exist(fb: FacebookInit) {
    let mut config = RepoConfig::default();

    config.bookmarks = vec![];
    config.hooks = vec![];

    let mut hm = hook_manager_many_files_dirs_repo(fb).await;

    match load_hooks(
        fb,
        &DefaultAclProvider::new(fb),
        &mut hm,
        &config,
        &hashset!["hook1".to_string()],
    )
    .await
    .unwrap_err()
    .downcast::<ErrorKind>()
    {
        Ok(ErrorKind::NoSuchHookToDisable(hooks)) => {
            assert_eq!(hashset!["hook1".to_string()], hooks);
        }
        _ => assert!(false, "Unexpected err type"),
    };
}

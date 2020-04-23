/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Error;
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{
    compat::Future01CompatExt,
    future,
    stream::{futures_unordered, TryStreamExt},
};
use hooks::{
    hook_loader::load_hooks, ChangesetHook, ErrorKind, FileHook, HookExecution, HookManager,
    HookRejectionInfo,
};
use hooks_content_stores::{
    BlobRepoFileContentFetcher, FileContentFetcher, InMemoryFileContentFetcher,
};
use maplit::{btreemap, hashmap, hashset};
use metaconfig_types::{BookmarkParams, HookConfig, HookParams, HookType, RepoConfig};
use mononoke_types::{BonsaiChangeset, BonsaiChangesetMut, DateTime, FileChange, FileType, MPath};
use mononoke_types_mocks::contentid::{ONES_CTID, THREES_CTID, TWOS_CTID};
use regex::Regex;
use scuba_ext::ScubaSampleBuilder;
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};
use tests_utils::{create_commit, store_files};

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
        _content_fetcher: &'fetcher dyn FileContentFetcher,
    ) -> Result<HookExecution, Error> {
        Ok((self.f)())
    }
}

fn always_accepting_changeset_hook() -> Box<dyn ChangesetHook> {
    let f: fn() -> HookExecution = || HookExecution::Accepted;
    Box::new(FnChangesetHook::new(f))
}

fn always_rejecting_changeset_hook() -> Box<dyn ChangesetHook> {
    let f: fn() -> HookExecution = || default_rejection();
    Box::new(FnChangesetHook::new(f))
}

#[derive(Clone, Debug)]
struct FileContentMatchingChangesetHook {
    expected_content: HashMap<MPath, Option<String>>,
}

#[async_trait]
impl ChangesetHook for FileContentMatchingChangesetHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        _bookmark: &BookmarkName,
        changeset: &'cs BonsaiChangeset,
        content_fetcher: &'fetcher dyn FileContentFetcher,
    ) -> Result<HookExecution, Error> {
        let futs = futures_unordered::FuturesUnordered::new();

        for (path, change) in changeset.file_changes() {
            // If we have a change to a path, but no expected change, fail
            let expected_content = match self.expected_content.get(path) {
                Some(expected) => expected,
                None => return Ok(default_rejection()),
            };

            match change {
                Some(change) => {
                    let fut = async move {
                        let content = content_fetcher
                            .get_file_text(ctx, change.content_id())
                            .await?;
                        let content =
                            content.map(|c| std::str::from_utf8(c.as_ref()).unwrap().to_string());

                        // True only if there is content containing the expected content
                        Ok(match (content, expected_content.as_ref()) {
                            (Some(content), Some(expected_content)) => {
                                if content.contains(expected_content) {
                                    true
                                } else {
                                    false
                                }
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
        content_fetcher: &'fetcher dyn FileContentFetcher,
    ) -> Result<HookExecution, Error> {
        let futs = futures_unordered::FuturesUnordered::new();
        for (path, change) in changeset.file_changes() {
            let expected_length = self.expected_lengths.get(path);

            match change {
                Some(change) => {
                    let fut = async move {
                        let size = content_fetcher
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
        _content_fetcher: &'fetcher dyn FileContentFetcher,
        _change: Option<&'change FileChange>,
        _path: &'path MPath,
    ) -> Result<HookExecution, Error> {
        Ok((self.f)())
    }
}

fn always_accepting_file_hook() -> Box<dyn FileHook> {
    let f: fn() -> HookExecution = || HookExecution::Accepted;
    Box::new(FnFileHook::new(f))
}

fn always_rejecting_file_hook() -> Box<dyn FileHook> {
    let f: fn() -> HookExecution = || default_rejection();
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
        _content_fetcher: &'fetcher dyn FileContentFetcher,
        _change: Option<&'change FileChange>,
        path: &'path MPath,
    ) -> Result<HookExecution, Error> {
        Ok(if self.paths.contains(&path) {
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
        content_fetcher: &'fetcher dyn FileContentFetcher,
        change: Option<&'change FileChange>,
        _path: &'path MPath,
    ) -> Result<HookExecution, Error> {
        match change {
            Some(change) => {
                let content = content_fetcher
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
        _content_fetcher: &'fetcher dyn FileContentFetcher,
        change: Option<&'change FileChange>,
        _path: &'path MPath,
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
        content_fetcher: &'fetcher dyn FileContentFetcher,
        change: Option<&'change FileChange>,
        _path: &'path MPath,
    ) -> Result<HookExecution, Error> {
        let length = match change {
            Some(change) => {
                content_fetcher
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
fn test_changeset_hook_accepted(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
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
    });
}

#[fbinit::test]
fn test_changeset_hook_rejected(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
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
    });
}

#[fbinit::test]
fn test_changeset_hook_mix(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
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
    });
}

#[fbinit::test]
fn test_changeset_hook_file_text(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
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
    });
}

#[fbinit::test]
fn test_changeset_hook_lengths(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
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
    });
}

#[fbinit::test]
fn test_file_hook_accepted(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
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
    });
}

#[fbinit::test]
fn test_file_hook_rejected(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
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
    });
}

#[fbinit::test]
fn test_file_hook_mix(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
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
    });
}

#[fbinit::test]
fn test_file_hooks_paths(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
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
    });
}

#[fbinit::test]
fn test_file_hooks_paths_mix(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
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
    });
}

#[fbinit::test]
fn test_file_hook_file_text(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
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
    });
}

#[fbinit::test]
fn test_file_hook_is_symlink(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
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
    });
}

#[fbinit::test]
fn test_file_hook_length(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
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
    });
}

#[fbinit::test]
fn test_cs_hooks_with_blob_store(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
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
            "bm1",
            hooks,
            bookmarks,
            regexes.clone(),
            expected,
            ContentFetcherType::Blob(fixtures::many_files_dirs::getrepo(ctx.fb).await),
        )
        .await;
    });
}

#[fbinit::test]
fn test_file_hooks_with_blob_store(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        // Create an init a repo
        let (repo, bcs_id) = {
            let repo = blobrepo_factory::new_memblob_empty(None).unwrap();

            let parent = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![],
                store_files(
                    ctx.clone(),
                    btreemap! {"toremove" => Some("content")},
                    repo.clone(),
                )
                .await,
            )
            .await;
            let bcs_id = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![parent],
                store_files(
                    ctx.clone(),
                    btreemap! {
                        "toremove" => None,
                        "newfile" => Some("newcontent"),
                        "dir/somefile" => Some("good"),
                    },
                    repo.clone(),
                )
                .await,
            )
            .await;

            let mut txn = repo.update_bookmark_transaction(ctx.clone());
            txn.force_set(
                &BookmarkName::new("master").unwrap(),
                bcs_id,
                BookmarkUpdateReason::TestMove {
                    bundle_replay_data: None,
                },
            )
            .unwrap();
            txn.commit().compat().await.unwrap();
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
            .load(ctx.clone(), &repo.get_blobstore())
            .compat()
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
    })
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
    bookmark_name: &str,
    hooks: HashMap<String, Box<dyn ChangesetHook>>,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
    expected: HashMap<String, HookExecution>,
    content_fetcher_type: ContentFetcherType,
) {
    let mut hook_manager =
        setup_hook_manager(ctx.fb, bookmarks, regexes, content_fetcher_type).await;
    for (hook_name, hook) in hooks {
        hook_manager.register_changeset_hook(&hook_name, hook, Default::default());
    }
    let res = hook_manager
        .run_hooks_for_bookmark(
            &ctx,
            vec![default_changeset()].iter(),
            &BookmarkName::new(bookmark_name).unwrap(),
            None,
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
    Blob(BlobRepo),
}

async fn run_file_hooks(
    ctx: CoreContext,
    bookmark_name: &str,
    hooks: HashMap<String, Box<dyn FileHook>>,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
    expected: HashMap<String, HashMap<String, HookExecution>>,
    content_fetcher_type: ContentFetcherType,
) {
    run_file_hooks_with_mgr(
        ctx,
        bookmark_name,
        hooks,
        bookmarks,
        regexes,
        expected,
        content_fetcher_type,
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
    content_fetcher_type: ContentFetcherType,
    cs: BonsaiChangeset,
) {
    run_file_hooks_with_mgr(
        ctx,
        bookmark_name,
        hooks,
        bookmarks,
        regexes,
        expected,
        content_fetcher_type,
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
    content_fetcher_type: ContentFetcherType,
    cs: BonsaiChangeset,
) {
    let mut hook_manager =
        setup_hook_manager(ctx.fb, bookmarks, regexes, content_fetcher_type).await;
    for (hook_name, hook) in hooks {
        hook_manager.register_file_hook(&hook_name, hook, Default::default());
    }
    let res = hook_manager
        .run_hooks_for_bookmark(
            &ctx,
            vec![cs].iter(),
            &BookmarkName::new(bookmark_name).unwrap(),
            None,
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
    content_fetcher_type: ContentFetcherType,
) -> HookManager {
    let mut hook_manager = match content_fetcher_type {
        ContentFetcherType::InMemory => hook_manager_inmem(fb).await,
        ContentFetcherType::Blob(repo) => hook_manager_blobrepo(fb, repo).await,
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
    HookExecution::Rejected(HookRejectionInfo::new_long(
        "desc".into(),
        "long_desc".to_string(),
    ))
}

fn default_changeset() -> BonsaiChangeset {
    BonsaiChangesetMut {
        parents: Vec::new(),
        author: "Jeremy Fitzhardinge <jsgf@fb.com>".to_string(),
        author_date: DateTime::from_timestamp(1584887580, 0).expect("Getting timestamp"),
        committer: None,
        committer_date: None,
        message: "This is a commit message".to_string(),
        extra: BTreeMap::new(),
        file_changes: btreemap!{
            to_mpath("dir1/subdir1/subsubdir1/file_1") => Some(FileChange::new(ONES_CTID, FileType::Symlink, 15, None)),
            to_mpath("dir1/subdir1/subsubdir2/file_1") => Some(FileChange::new(TWOS_CTID, FileType::Regular, 17, None)),
            to_mpath("dir1/subdir1/subsubdir2/file_2") => Some(FileChange::new(THREES_CTID, FileType::Regular, 2, None)),
        },
    }.freeze().expect("Created changeset")
}

async fn hook_manager_blobrepo(fb: FacebookInit, repo: BlobRepo) -> HookManager {
    let ctx = CoreContext::test_mock(fb);

    let content_fetcher = BlobRepoFileContentFetcher::new(repo);
    HookManager::new(
        ctx.fb,
        Box::new(content_fetcher),
        Default::default(),
        ScubaSampleBuilder::with_discard(),
    )
    .await
    .expect("Failed to construct HookManager")
}

async fn hook_manager_many_files_dirs_blobrepo(fb: FacebookInit) -> HookManager {
    hook_manager_blobrepo(fb, fixtures::many_files_dirs::getrepo(fb).await).await
}

fn to_mpath(string: &str) -> MPath {
    // Please... avert your eyes
    MPath::new(string.to_string().as_bytes().to_vec()).unwrap()
}

async fn hook_manager_inmem(fb: FacebookInit) -> HookManager {
    let ctx = CoreContext::test_mock(fb);

    let mut content_fetcher = InMemoryFileContentFetcher::new();
    content_fetcher.insert(ONES_CTID, "elephants");
    content_fetcher.insert(TWOS_CTID, "hippopatami");
    content_fetcher.insert(THREES_CTID, "eels");

    HookManager::new(
        ctx.fb,
        Box::new(content_fetcher),
        Default::default(),
        ScubaSampleBuilder::with_discard(),
    )
    .await
    .expect("Failed to construct HookManager")
}

#[fbinit::test]
fn test_verify_integrity_fast_failure(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let mut config = RepoConfig::default();
        config.bookmarks = vec![BookmarkParams {
            bookmark: Regex::new("bm2").unwrap().into(),
            hooks: vec!["rust:verify_integrity".into()],
            only_fast_forward: false,
            allowed_users: None,
            rewrite_dates: None,
        }];
        config.hooks = vec![HookParams {
            name: "rust:verify_integrity".into(),
            hook_type: HookType::PerChangeset,
            config: HookConfig {
                strings: hashmap! {String::from("verify_integrity_path") => String::from("bad_nonexisting_filename")},
                ..Default::default()
            },
        }];

        let mut hm = hook_manager_many_files_dirs_blobrepo(fb).await;
        load_hooks(fb, &mut hm, config, &hashset![])
            .expect_err("`verify_integrity` hook loading should have failed");
    });
}

#[fbinit::test]
fn test_load_hooks_bad_rust_hook(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let mut config = RepoConfig::default();
        config.bookmarks = vec![BookmarkParams {
            bookmark: BookmarkName::new("bm1").unwrap().into(),
            hooks: vec!["rust:hook1".into()],
            only_fast_forward: false,
            allowed_users: None,
            rewrite_dates: None,
        }];

        config.hooks = vec![HookParams {
            name: "rust:hook1".into(),
            hook_type: HookType::PerChangeset,
            config: Default::default(),
        }];

        let mut hm = hook_manager_many_files_dirs_blobrepo(fb).await;

        match load_hooks(fb, &mut hm, config, &hashset![])
            .unwrap_err()
            .downcast::<ErrorKind>()
        {
            Ok(ErrorKind::InvalidRustHook(hook_name)) => {
                assert_eq!(hook_name, "rust:hook1".to_string());
            }
            _ => assert!(false, "Unexpected err type"),
        };
    });
}

#[fbinit::test]
fn test_load_disabled_hooks(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let mut config = RepoConfig::default();

        config.bookmarks = vec![];

        config.hooks = vec![HookParams {
            name: "hook1".into(),
            hook_type: HookType::PerChangeset,
            config: Default::default(),
        }];

        let mut hm = hook_manager_many_files_dirs_blobrepo(fb).await;

        load_hooks(fb, &mut hm, config, &hashset!["hook1".to_string()])
            .expect("disabling a broken hook should allow loading to succeed");
    });
}

#[fbinit::test]
fn test_load_disabled_hooks_referenced_by_bookmark(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let mut config = RepoConfig::default();

        config.bookmarks = vec![BookmarkParams {
            bookmark: BookmarkName::new("bm1").unwrap().into(),
            hooks: vec!["hook1".into()],
            only_fast_forward: false,
            allowed_users: None,
            rewrite_dates: None,
        }];

        config.hooks = vec![HookParams {
            name: "hook1".into(),
            hook_type: HookType::PerChangeset,
            config: Default::default(),
        }];

        let mut hm = hook_manager_many_files_dirs_blobrepo(fb).await;

        load_hooks(fb, &mut hm, config, &hashset!["hook1".to_string()])
            .expect("disabling a broken hook should allow loading to succeed");
    });
}

#[fbinit::test]
fn test_load_disabled_hooks_hook_does_not_exist(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let mut config = RepoConfig::default();

        config.bookmarks = vec![];
        config.hooks = vec![];

        let mut hm = hook_manager_many_files_dirs_blobrepo(fb).await;

        match load_hooks(fb, &mut hm, config, &hashset!["hook1".to_string()])
            .unwrap_err()
            .downcast::<ErrorKind>()
        {
            Ok(ErrorKind::NoSuchHookToDisable(hooks)) => {
                assert_eq!(hashset!["hook1".to_string()], hooks);
            }
            _ => assert!(false, "Unexpected err type"),
        };
    });
}

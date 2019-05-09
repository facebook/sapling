// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use bookmarks::Bookmark;
use context::CoreContext;
use failure_ext::Error;
use fixtures::many_files_dirs;
use futures::future::finished;
use futures::Future;
use futures::{stream, Stream};
use futures_ext::{BoxFuture, FutureExt};
use hooks::{
    hook_loader::load_hooks, ChangedFileType, ErrorKind, FileHookExecutionID, Hook, HookChangeset,
    HookChangesetParents, HookContext, HookExecution, HookFile, HookManager, HookRejectionInfo,
};
use hooks::{InMemoryChangesetStore, InMemoryFileContentStore};
use hooks_content_stores::{BlobRepoChangesetStore, BlobRepoFileContentStore};
use maplit::{hashmap, hashset};
use mercurial_types::{HgChangesetId, MPath};
use metaconfig_types::{
    BlobConfig, BookmarkOrRegex, BookmarkParams, Bundle2ReplayParams, HookConfig, HookParams,
    HookType, MetadataDBConfig, RepoConfig, RepoReadOnly, StorageConfig,
};
use mononoke_types::FileType;
use regex::Regex;
use slog::{o, Logger};
use slog::{Discard, Drain};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

#[derive(Clone, Debug)]
struct FnChangesetHook {
    f: fn(HookContext<HookChangeset>) -> HookExecution,
}

impl FnChangesetHook {
    fn new(f: fn(HookContext<HookChangeset>) -> HookExecution) -> FnChangesetHook {
        FnChangesetHook { f }
    }
}

impl Hook<HookChangeset> for FnChangesetHook {
    fn run(
        &self,
        _ctx: CoreContext,
        context: HookContext<HookChangeset>,
    ) -> BoxFuture<HookExecution, Error> {
        finished((self.f)(context)).boxify()
    }
}

fn always_accepting_changeset_hook() -> Box<Hook<HookChangeset>> {
    let f: fn(HookContext<HookChangeset>) -> HookExecution = |_| HookExecution::Accepted;
    Box::new(FnChangesetHook::new(f))
}

fn always_rejecting_changeset_hook() -> Box<Hook<HookChangeset>> {
    let f: fn(HookContext<HookChangeset>) -> HookExecution = |_| default_rejection();
    Box::new(FnChangesetHook::new(f))
}

#[derive(Clone, Debug)]
struct ContextMatchingChangesetHook {
    expected_context: HookContext<HookChangeset>,
}

impl Hook<HookChangeset> for ContextMatchingChangesetHook {
    fn run(
        &self,
        _ctx: CoreContext,
        context: HookContext<HookChangeset>,
    ) -> BoxFuture<HookExecution, Error> {
        assert_eq!(self.expected_context, context);
        Box::new(finished(HookExecution::Accepted))
    }
}

fn context_matching_changeset_hook(
    expected_context: HookContext<HookChangeset>,
) -> Box<Hook<HookChangeset>> {
    Box::new(ContextMatchingChangesetHook { expected_context })
}

#[derive(Clone, Debug)]
struct ContainsStringMatchingChangesetHook {
    expected_content: HashMap<String, String>,
}

impl Hook<HookChangeset> for ContainsStringMatchingChangesetHook {
    fn run(
        &self,
        ctx: CoreContext,
        context: HookContext<HookChangeset>,
    ) -> BoxFuture<HookExecution, Error> {
        let mut futs = stream::FuturesUnordered::new();
        for file in context.data.files {
            let fut = match self.expected_content.get(&file.path) {
                Some(content) => file.contains_string(ctx.clone(), &content),
                None => Box::new(finished(false)),
            };
            futs.push(fut);
        }
        futs.skip_while(|b| Ok(*b))
            .into_future()
            .map(|(opt_item, _)| {
                if opt_item.is_some() {
                    default_rejection()
                } else {
                    HookExecution::Accepted
                }
            })
            .map_err(|(e, _)| e)
            .boxify()
    }
}

fn contains_string_matching_changeset_hook(
    expected_content: HashMap<String, String>,
) -> Box<Hook<HookChangeset>> {
    Box::new(ContainsStringMatchingChangesetHook { expected_content })
}

#[derive(Clone, Debug)]
struct FileContentMatchingChangesetHook {
    expected_content: HashMap<String, String>,
}

impl Hook<HookChangeset> for FileContentMatchingChangesetHook {
    fn run(
        &self,
        ctx: CoreContext,
        context: HookContext<HookChangeset>,
    ) -> BoxFuture<HookExecution, Error> {
        let mut futs = stream::FuturesUnordered::new();
        for file in context.data.files {
            let fut = match self.expected_content.get(&file.path) {
                Some(expected_content) => {
                    let expected_content = expected_content.clone();
                    file.file_content(ctx.clone())
                        .map(move |content| {
                            let content = std::str::from_utf8(&*content).unwrap().to_string();
                            content.contains(&expected_content)
                        })
                        .boxify()
                }
                None => Box::new(finished(false)),
            };
            futs.push(fut);
        }
        futs.skip_while(|b| Ok(*b))
            .into_future()
            .map(|(opt_item, _)| {
                if opt_item.is_some() {
                    default_rejection()
                } else {
                    HookExecution::Accepted
                }
            })
            .map_err(|(e, _)| e)
            .boxify()
    }
}

fn file_content_matching_changeset_hook(
    expected_content: HashMap<String, String>,
) -> Box<Hook<HookChangeset>> {
    Box::new(FileContentMatchingChangesetHook { expected_content })
}

#[derive(Clone, Debug)]
struct LengthMatchingChangesetHook {
    expected_lengths: HashMap<String, u64>,
}

impl Hook<HookChangeset> for LengthMatchingChangesetHook {
    fn run(
        &self,
        ctx: CoreContext,
        context: HookContext<HookChangeset>,
    ) -> BoxFuture<HookExecution, Error> {
        let mut futs = stream::FuturesUnordered::new();
        for file in context.data.files {
            let fut = match self.expected_lengths.get(&file.path) {
                Some(expected_length) => {
                    let expected_length = *expected_length;
                    file.len(ctx.clone())
                        .map(move |length| length == expected_length)
                        .boxify()
                }
                None => Box::new(finished(false)),
            };
            futs.push(fut);
        }
        futs.skip_while(|b| Ok(*b))
            .into_future()
            .map(|(opt_item, _)| {
                if opt_item.is_some() {
                    default_rejection()
                } else {
                    HookExecution::Accepted
                }
            })
            .map_err(|(e, _)| e)
            .boxify()
    }
}

fn length_matching_changeset_hook(
    expected_lengths: HashMap<String, u64>,
) -> Box<Hook<HookChangeset>> {
    Box::new(LengthMatchingChangesetHook { expected_lengths })
}

#[derive(Clone, Debug)]
struct OtherFileMatchingChangesetHook {
    file_path: String,
    expected_content: Option<String>,
}

impl Hook<HookChangeset> for OtherFileMatchingChangesetHook {
    fn run(
        &self,
        ctx: CoreContext,
        context: HookContext<HookChangeset>,
    ) -> BoxFuture<HookExecution, Error> {
        let expected_content = self.expected_content.clone();
        context
            .data
            .file_content(ctx, self.file_path.clone())
            .map(|opt| opt.map(|content| std::str::from_utf8(&*content).unwrap().to_string()))
            .map(move |opt| {
                if opt == expected_content {
                    HookExecution::Accepted
                } else {
                    default_rejection()
                }
            })
            .boxify()
    }
}

fn other_file_matching_changeset_hook(
    file_path: String,
    expected_content: Option<String>,
) -> Box<Hook<HookChangeset>> {
    Box::new(OtherFileMatchingChangesetHook {
        file_path,
        expected_content,
    })
}

#[derive(Clone, Debug)]
struct FnFileHook {
    f: fn(HookContext<HookFile>) -> HookExecution,
}

impl FnFileHook {
    fn new(f: fn(HookContext<HookFile>) -> HookExecution) -> FnFileHook {
        FnFileHook { f }
    }
}

impl Hook<HookFile> for FnFileHook {
    fn run(
        &self,
        _ctx: CoreContext,
        context: HookContext<HookFile>,
    ) -> BoxFuture<HookExecution, Error> {
        finished((self.f)(context)).boxify()
    }
}

fn always_accepting_file_hook() -> Box<Hook<HookFile>> {
    let f: fn(HookContext<HookFile>) -> HookExecution = |_| HookExecution::Accepted;
    Box::new(FnFileHook::new(f))
}

fn always_rejecting_file_hook() -> Box<Hook<HookFile>> {
    let f: fn(HookContext<HookFile>) -> HookExecution = |_| default_rejection();
    Box::new(FnFileHook::new(f))
}

#[derive(Clone, Debug)]
struct PathMatchingFileHook {
    paths: HashSet<String>,
}

impl Hook<HookFile> for PathMatchingFileHook {
    fn run(
        &self,
        _ctx: CoreContext,
        context: HookContext<HookFile>,
    ) -> BoxFuture<HookExecution, Error> {
        finished(if self.paths.contains(&context.data.path) {
            HookExecution::Accepted
        } else {
            default_rejection()
        })
        .boxify()
    }
}

fn path_matching_file_hook(paths: HashSet<String>) -> Box<Hook<HookFile>> {
    Box::new(PathMatchingFileHook { paths })
}

#[derive(Clone, Debug)]
struct ContainsStringMatchingFileHook {
    content: String,
}

impl Hook<HookFile> for ContainsStringMatchingFileHook {
    fn run(
        &self,
        ctx: CoreContext,
        context: HookContext<HookFile>,
    ) -> BoxFuture<HookExecution, Error> {
        context
            .data
            .contains_string(ctx, &self.content)
            .map(|contains| {
                if contains {
                    HookExecution::Accepted
                } else {
                    default_rejection()
                }
            })
            .boxify()
    }
}

fn contains_string_matching_file_hook(content: String) -> Box<Hook<HookFile>> {
    Box::new(ContainsStringMatchingFileHook { content })
}

#[derive(Clone, Debug)]
struct FileContentMatchingFileHook {
    content: String,
}

impl Hook<HookFile> for FileContentMatchingFileHook {
    fn run(
        &self,
        ctx: CoreContext,
        context: HookContext<HookFile>,
    ) -> BoxFuture<HookExecution, Error> {
        let expected_content = self.content.clone();
        context
            .data
            .file_content(ctx)
            .map(move |content| {
                let content = std::str::from_utf8(&*content).unwrap().to_string();
                if content.contains(&expected_content) {
                    HookExecution::Accepted
                } else {
                    default_rejection()
                }
            })
            .boxify()
    }
}

fn file_content_matching_file_hook(content: String) -> Box<Hook<HookFile>> {
    Box::new(FileContentMatchingFileHook { content })
}

#[derive(Clone, Debug)]
struct IsSymLinkMatchingFileHook {
    is_symlink: bool,
}

impl Hook<HookFile> for IsSymLinkMatchingFileHook {
    fn run(
        &self,
        ctx: CoreContext,
        context: HookContext<HookFile>,
    ) -> BoxFuture<HookExecution, Error> {
        let is_symlink = self.is_symlink;
        context
            .data
            .file_type(ctx)
            .map(move |file_type| {
                let actual = match file_type {
                    FileType::Symlink => true,
                    _ => false,
                };
                if is_symlink == actual {
                    HookExecution::Accepted
                } else {
                    default_rejection()
                }
            })
            .boxify()
    }
}

fn is_symlink_matching_file_hook(is_symlink: bool) -> Box<Hook<HookFile>> {
    Box::new(IsSymLinkMatchingFileHook { is_symlink })
}

#[derive(Clone, Debug)]
struct LengthMatchingFileHook {
    length: u64,
}

impl Hook<HookFile> for LengthMatchingFileHook {
    fn run(
        &self,
        ctx: CoreContext,
        context: HookContext<HookFile>,
    ) -> BoxFuture<HookExecution, Error> {
        let exp_length = self.length;
        context
            .data
            .len(ctx)
            .map(move |length| {
                if length == exp_length {
                    HookExecution::Accepted
                } else {
                    default_rejection()
                }
            })
            .boxify()
    }
}

fn length_matching_file_hook(length: u64) -> Box<Hook<HookFile>> {
    Box::new(LengthMatchingFileHook { length })
}

#[test]
fn test_changeset_hook_accepted() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
            "hook1".to_string() => always_accepting_changeset_hook()
        };
        let bookmarks = hashmap! {
            "bm1".to_string() => vec!["hook1".to_string()]
        };
        let regexes = hashmap! {};
        let expected = hashmap! {
            "hook1".to_string() => HookExecution::Accepted
        };
        run_changeset_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_changeset_hook_rejected() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
            "hook1".to_string() => always_rejecting_changeset_hook()
        };
        let bookmarks = hashmap! {
            "bm1".to_string() => vec!["hook1".to_string()]
        };
        let regexes = hashmap! {};
        let expected = hashmap! {
            "hook1".to_string() => default_rejection()
        };
        run_changeset_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_changeset_hook_mix() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
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
        run_changeset_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_changeset_hook_context() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let files = vec![
            "dir1/subdir1/subsubdir1/file_1".to_string(),
            "dir1/subdir1/subsubdir2/file_1".to_string(),
            "dir1/subdir1/subsubdir2/file_2".to_string(),
        ];
        let content_store = Arc::new(InMemoryFileContentStore::new());
        let cs_id = default_changeset_id();
        let hook_files = files
            .iter()
            .map(|path| {
                HookFile::new(
                    path.clone(),
                    content_store.clone(),
                    cs_id,
                    ChangedFileType::Added,
                )
            })
            .collect();
        let parents = HookChangesetParents::One("2f866e7e549760934e31bf0420a873f65100ad63".into());
        let reviewers_acl_checker = Arc::new(None);
        let data = HookChangeset::new(
            "Stanislau Hlebik <stash@fb.com>".into(),
            hook_files,
            "3".into(),
            parents,
            cs_id,
            content_store,
            reviewers_acl_checker,
        );
        let expected_context = HookContext {
            hook_name: "hook1".into(),
            config: Default::default(),
            data,
            bookmark: Bookmark::new("bm1").unwrap(),
        };
        let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
            "hook1".to_string() => context_matching_changeset_hook(expected_context)
        };
        let bookmarks = hashmap! {
            "bm1".to_string() => vec!["hook1".to_string()]
        };
        let regexes = hashmap! {};
        let expected = hashmap! {
            "hook1".to_string() => HookExecution::Accepted
        };
        run_changeset_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_changeset_hook_contains_string() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let hook1_map = hashmap![
            "dir1/subdir1/subsubdir1/file_1".to_string() => "elephants".to_string(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => "hippopatami".to_string(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => "eels".to_string()
        ];
        let hook2_map = hashmap![
            "dir1/subdir1/subsubdir1/file_1".to_string() => "anteaters".to_string(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => "hippopatami".to_string(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => "eels".to_string()
        ];
        let hook3_map = hashmap![
            "dir1/subdir1/subsubdir1/file_1".to_string() => "anteaters".to_string(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => "giraffes".to_string(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => "lions".to_string()
        ];
        let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
            "hook1".to_string() => contains_string_matching_changeset_hook(hook1_map),
            "hook2".to_string() => contains_string_matching_changeset_hook(hook2_map),
            "hook3".to_string() => contains_string_matching_changeset_hook(hook3_map),
        };
        let bookmarks = hashmap! {
            "bm1".to_string() => vec!["hook1".to_string(), "hook2".to_string()]
        };
        let regexes = hashmap! {
            "b.*".to_string() => vec!["hook3".to_string()]
        };
        let expected = hashmap! {
            "hook1".to_string() => HookExecution::Accepted,
            "hook2".to_string() => default_rejection(),
            "hook3".to_string() => default_rejection(),
        };
        run_changeset_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_changeset_hook_other_file_content() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
            "hook1".to_string() => other_file_matching_changeset_hook("dir1/subdir1/subsubdir1/file_1".to_string(), Some("elephants".to_string())),
            "hook2".to_string() => other_file_matching_changeset_hook("dir1/subdir1/subsubdir1/file_1".to_string(), Some("giraffes".to_string())),
            "hook3".to_string() => other_file_matching_changeset_hook("dir1/subdir1/subsubdir2/file_2".to_string(), Some("aardvarks".to_string())),
            "hook4".to_string() => other_file_matching_changeset_hook("no/such/path".to_string(), None),
            "hook5".to_string() => other_file_matching_changeset_hook("no/such/path".to_string(), Some("whateva".to_string())),
        };
        let bookmarks = hashmap! {
            "bm1".to_string() => vec!["hook1".to_string(), "hook2".to_string(), "hook3".to_string()]
        };
        let regexes = hashmap! {
            "b.*".to_string() => vec!["hook3".to_string(), "hook4".to_string(), "hook5".to_string()]
        };
        let expected = hashmap! {
            "hook1".to_string() => HookExecution::Accepted,
            "hook2".to_string() => default_rejection(),
            "hook3".to_string() => default_rejection(),
            "hook4".to_string() => HookExecution::Accepted,
            "hook5".to_string() => default_rejection(),
        };
        run_changeset_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_changeset_hook_file_content() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let hook1_map = hashmap![
            "dir1/subdir1/subsubdir1/file_1".to_string() => "elephants".to_string(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => "hippopatami".to_string(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => "eels".to_string()
        ];
        let hook2_map = hashmap![
            "dir1/subdir1/subsubdir1/file_1".to_string() => "anteaters".to_string(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => "hippopatami".to_string(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => "eels".to_string()
        ];
        let hook3_map = hashmap![
            "dir1/subdir1/subsubdir1/file_1".to_string() => "anteaters".to_string(),
            "dir1/subdir1/subsubdir2/file_1".to_string() => "giraffes".to_string(),
            "dir1/subdir1/subsubdir2/file_2".to_string() => "lions".to_string()
        ];
        let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
            "hook1".to_string() => file_content_matching_changeset_hook(hook1_map),
            "hook2".to_string() => file_content_matching_changeset_hook(hook2_map),
            "hook3".to_string() => file_content_matching_changeset_hook(hook3_map),
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
        run_changeset_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_changeset_hook_lengths() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let hook1_map = hashmap![
            "dir1/subdir1/subsubdir1/file_1".to_string() => 9,
            "dir1/subdir1/subsubdir2/file_1".to_string() => 11,
            "dir1/subdir1/subsubdir2/file_2".to_string() => 4
        ];
        let hook2_map = hashmap![
            "dir1/subdir1/subsubdir1/file_1".to_string() => 9,
            "dir1/subdir1/subsubdir2/file_1".to_string() => 12,
            "dir1/subdir1/subsubdir2/file_2".to_string() => 4
        ];
        let hook3_map = hashmap![
            "dir1/subdir1/subsubdir1/file_1".to_string() => 15,
            "dir1/subdir1/subsubdir2/file_1".to_string() => 17,
            "dir1/subdir1/subsubdir2/file_2".to_string() => 2
        ];
        let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
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
        run_changeset_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_file_hook_accepted() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
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
        run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_file_hook_rejected() {
    async_unit::tokio_unit_test(move || {
        let ctx = CoreContext::test_mock();
        let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
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
        run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_file_hook_mix() {
    async_unit::tokio_unit_test(move || {
        let ctx = CoreContext::test_mock();
        let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
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
        run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_file_hooks_paths() {
    async_unit::tokio_unit_test(move || {
        let ctx = CoreContext::test_mock();
        let matching_paths = hashset![
            "dir1/subdir1/subsubdir2/file_1".to_string(),
            "dir1/subdir1/subsubdir2/file_2".to_string(),
        ];
        let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
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
        run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_file_hooks_paths_mix() {
    async_unit::tokio_unit_test(move || {
        let ctx = CoreContext::test_mock();
        let matching_paths1 = hashset![
            "dir1/subdir1/subsubdir2/file_1".to_string(),
            "dir1/subdir1/subsubdir2/file_2".to_string(),
        ];
        let matching_paths2 = hashset!["dir1/subdir1/subsubdir1/file_1".to_string(),];
        let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
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
        run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_file_hook_contains_string() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
            "hook1".to_string() => contains_string_matching_file_hook("elephants".to_string()),
            "hook2".to_string() => contains_string_matching_file_hook("hippopatami".to_string()),
            "hook3".to_string() => contains_string_matching_file_hook("eels".to_string())
        };
        let bookmarks = hashmap! {
            "bm1".to_string() => vec!["hook1".to_string(), "hook2".to_string()],
        };
        let regexes = hashmap! {
            "^b.*$".to_string() => vec!["hook3".to_string()],
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
        run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_file_hook_file_content() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
            "hook1".to_string() => file_content_matching_file_hook("elephants".to_string()),
            "hook2".to_string() => file_content_matching_file_hook("hippopatami".to_string()),
            "hook3".to_string() => file_content_matching_file_hook("eels".to_string())
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
        run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_file_hook_is_symlink() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
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
        run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_file_hook_length() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let hooks: HashMap<String, Box<Hook<HookFile>>> = hashmap! {
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
        run_file_hooks(ctx, "bm1", hooks, bookmarks, regexes, expected);
    });
}

#[test]
fn test_register_changeset_hooks() {
    async_unit::tokio_unit_test(|| {
        let mut hook_manager = hook_manager_inmem();
        let hook1 = always_accepting_changeset_hook();
        hook_manager.register_changeset_hook("hook1", hook1.into(), Default::default());
        let hook2 = always_accepting_changeset_hook();
        hook_manager.register_changeset_hook("hook2", hook2.into(), Default::default());

        let set = hook_manager.changeset_hook_names();
        assert_eq!(2, set.len());
        assert!(set.contains("hook1"));
        assert!(set.contains("hook1"));
    });
}

#[test]
fn test_with_blob_store() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let hooks: HashMap<String, Box<Hook<HookChangeset>>> = hashmap! {
            "hook1".to_string() => always_accepting_changeset_hook()
        };
        let bookmarks = hashmap! {
            "bm1".to_string() => vec!["hook1".to_string()]
        };
        let regexes = hashmap! {};
        let expected = hashmap! {
            "hook1".to_string() => HookExecution::Accepted
        };
        run_changeset_hooks_with_mgr(ctx, "bm1", hooks, bookmarks, regexes, expected, false);
    });
}

fn run_changeset_hooks(
    ctx: CoreContext,
    bookmark_name: &str,
    hooks: HashMap<String, Box<Hook<HookChangeset>>>,
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
        true,
    )
}

fn run_changeset_hooks_with_mgr(
    ctx: CoreContext,
    bookmark_name: &str,
    hooks: HashMap<String, Box<Hook<HookChangeset>>>,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
    expected: HashMap<String, HookExecution>,
    inmem: bool,
) {
    let mut hook_manager = setup_hook_manager(bookmarks, regexes, inmem);
    for (hook_name, hook) in hooks {
        hook_manager.register_changeset_hook(&hook_name, hook.into(), Default::default());
    }
    let fut = hook_manager.run_changeset_hooks_for_bookmark(
        ctx,
        default_changeset_id(),
        &Bookmark::new(bookmark_name).unwrap(),
        None,
    );
    let res = fut.wait().unwrap();
    let map: HashMap<String, HookExecution> = res
        .into_iter()
        .map(|(exec_id, exec)| (exec_id.hook_name, exec))
        .collect();
    assert_eq!(expected, map);
}

fn run_file_hooks(
    ctx: CoreContext,
    bookmark_name: &str,
    hooks: HashMap<String, Box<Hook<HookFile>>>,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
    expected: HashMap<String, HashMap<String, HookExecution>>,
) {
    run_file_hooks_with_mgr(
        ctx,
        bookmark_name,
        hooks,
        bookmarks,
        regexes,
        expected,
        true,
    )
}

fn run_file_hooks_with_mgr(
    ctx: CoreContext,
    bookmark_name: &str,
    hooks: HashMap<String, Box<Hook<HookFile>>>,
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
    expected: HashMap<String, HashMap<String, HookExecution>>,
    inmem: bool,
) {
    let mut hook_manager = setup_hook_manager(bookmarks, regexes, inmem);
    for (hook_name, hook) in hooks {
        hook_manager.register_file_hook(&hook_name, hook.into(), Default::default());
    }
    let fut: BoxFuture<Vec<(FileHookExecutionID, HookExecution)>, Error> = hook_manager
        .run_file_hooks_for_bookmark(
            ctx,
            default_changeset_id(),
            &Bookmark::new(bookmark_name).unwrap(),
            None,
        );
    let res = fut.wait().unwrap();
    let map: HashMap<String, HashMap<String, HookExecution>> =
        res.into_iter()
            .fold(HashMap::new(), |mut m, (exec_id, exec)| {
                match m.entry(exec_id.hook_name) {
                    Entry::Vacant(v) => v.insert(HashMap::new()).insert(exec_id.file.path, exec),
                    Entry::Occupied(mut v) => v.get_mut().insert(exec_id.file.path, exec),
                };
                m
            });
    assert_eq!(expected, map);
}

fn setup_hook_manager(
    bookmarks: HashMap<String, Vec<String>>,
    regexes: HashMap<String, Vec<String>>,
    inmem: bool,
) -> HookManager {
    let mut hook_manager = if inmem {
        hook_manager_inmem()
    } else {
        hook_manager_blobrepo()
    };
    for (bookmark_name, hook_names) in bookmarks {
        hook_manager
            .set_hooks_for_bookmark(Bookmark::new(bookmark_name).unwrap().into(), hook_names);
    }
    for (regx, hook_names) in regexes {
        hook_manager.set_hooks_for_bookmark(Regex::new(&regx).unwrap().into(), hook_names);
    }
    hook_manager
}

fn default_rejection() -> HookExecution {
    HookExecution::Rejected(HookRejectionInfo::new("desc".into(), "long_desc".into()))
}

fn default_changeset_id() -> HgChangesetId {
    HgChangesetId::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4").unwrap()
}

fn hook_manager_blobrepo() -> HookManager {
    let ctx = CoreContext::test_mock();
    let repo = many_files_dirs::getrepo(None);
    let changeset_store = BlobRepoChangesetStore::new(repo.clone());
    let content_store = BlobRepoFileContentStore::new(repo);
    let logger = Logger::root(Discard {}.ignore_res(), o!());
    HookManager::new(
        ctx,
        Box::new(changeset_store),
        Arc::new(content_store),
        Default::default(),
        logger,
    )
}

fn hook_manager_inmem() -> HookManager {
    let ctx = CoreContext::test_mock();
    let repo = many_files_dirs::getrepo(None);
    // Load up an in memory store with a single commit from the many_files_dirs store
    let cs_id = HgChangesetId::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4").unwrap();
    let cs = repo
        .get_changeset_by_changesetid(ctx.clone(), cs_id)
        .wait()
        .unwrap();
    let mut changeset_store = InMemoryChangesetStore::new();
    changeset_store.insert(cs_id, &cs);
    let mut content_store = InMemoryFileContentStore::new();
    content_store.insert(
        (cs_id.clone(), to_mpath("dir1/subdir1/subsubdir1/file_1")),
        (FileType::Symlink, "elephants".into()),
    );
    content_store.insert(
        (cs_id.clone(), to_mpath("dir1/subdir1/subsubdir2/file_1")),
        (FileType::Regular, "hippopatami".into()),
    );
    content_store.insert(
        (cs_id.clone(), to_mpath("dir1/subdir1/subsubdir2/file_2")),
        (FileType::Regular, "eels".into()),
    );
    let logger = Logger::root(Discard {}.ignore_res(), o!());
    HookManager::new(
        ctx,
        Box::new(changeset_store),
        Arc::new(content_store),
        Default::default(),
        logger,
    )
}

fn to_mpath(string: &str) -> MPath {
    // Please... avert your eyes
    MPath::new(string.to_string().as_bytes().to_vec()).unwrap()
}

fn default_repo_config() -> RepoConfig {
    RepoConfig {
        storage_config: StorageConfig {
            blobstore: BlobConfig::Disabled,
            dbconfig: MetadataDBConfig::LocalDB {
                path: "/some/place".into(),
            },
        },
        write_lock_db_address: None,
        enabled: true,
        generation_cache_size: 1,
        repoid: 1,
        scuba_table: None,
        cache_warmup: None,
        hook_manager_params: None,
        bookmarks_cache_ttl: None,
        bookmarks: vec![],
        hooks: vec![],
        pushrebase: Default::default(),
        lfs: Default::default(),
        wireproto_scribe_category: None,
        hash_validation_percentage: 0,
        readonly: RepoReadOnly::ReadWrite,
        skiplist_index_blobstore_key: None,
        bundle2_replay_params: Bundle2ReplayParams::default(),
    }
}

#[test]
fn test_load_hooks() {
    async_unit::tokio_unit_test(|| {
        let mut config = default_repo_config();
        config.bookmarks = vec![
            BookmarkParams {
                bookmark: Bookmark::new("bm1").unwrap().into(),
                hooks: vec!["hook1".into(), "hook2".into()],
                only_fast_forward: false,
                allowed_users: None,
            },
            BookmarkParams {
                bookmark: Regex::new("bm2").unwrap().into(),
                hooks: vec!["hook2".into(), "hook3".into(), "rust:restrict_users".into()],
                only_fast_forward: false,
                allowed_users: None,
            },
        ];

        config.hooks = vec![
            HookParams {
                name: "hook1".into(),
                code: Some("hook1 code".into()),
                hook_type: HookType::PerAddedOrModifiedFile,
                config: Default::default(),
            },
            HookParams {
                name: "hook2".into(),
                code: Some("hook2 code".into()),
                hook_type: HookType::PerAddedOrModifiedFile,
                config: Default::default(),
            },
            HookParams {
                name: "hook3".into(),
                code: Some("hook3 code".into()),
                hook_type: HookType::PerChangeset,
                config: Default::default(),
            },
            HookParams {
                name: "rust:restrict_users".into(),
                code: Some("whateva".into()),
                hook_type: HookType::PerChangeset,
                config: HookConfig {
                    strings: hashmap! {String::from("allow_users_regex") => String::from(".*")},
                    ..Default::default()
                },
            },
        ];

        let mut hm = hook_manager_blobrepo();
        match load_hooks(&mut hm, config) {
            Err(e) => assert!(false, format!("Failed to load hooks {}", e)),
            Ok(()) => (),
        };
    });
}

#[test]
fn test_verify_integrity_fast_failure() {
    let mut config = default_repo_config();
    config.bookmarks = vec![BookmarkParams {
        bookmark: Regex::new("bm2").unwrap().into(),
        hooks: vec!["rust:verify_integrity".into()],
        only_fast_forward: false,
        allowed_users: None,
    }];
    config.hooks = vec![HookParams {
        name: "rust:verify_integrity".into(),
        code: Some("whateva".into()),
        hook_type: HookType::PerChangeset,
        config: HookConfig {
            strings: hashmap! {String::from("verify_integrity_path") => String::from("bad_nonexisting_filename")},
            ..Default::default()
        },
    }];

    let mut hm = hook_manager_blobrepo();
    load_hooks(&mut hm, config).expect_err("`verify_integrity` hook loading should have failed");
}

#[test]
fn test_load_hooks_no_such_hook() {
    async_unit::tokio_unit_test(|| {
        let book_or_rex = BookmarkOrRegex::Bookmark(Bookmark::new("bm1").unwrap());
        let mut config = default_repo_config();
        config.bookmarks = vec![BookmarkParams {
            bookmark: book_or_rex.clone(),
            hooks: vec!["hook1".into(), "hook2".into()],
            only_fast_forward: false,
            allowed_users: None,
        }];

        config.hooks = vec![HookParams {
            name: "hook1".into(),
            code: Some("hook1 code".into()),
            hook_type: HookType::PerAddedOrModifiedFile,
            config: Default::default(),
        }];

        let mut hm = hook_manager_blobrepo();

        match load_hooks(&mut hm, config)
            .unwrap_err()
            .downcast::<ErrorKind>()
        {
            Ok(ErrorKind::NoSuchBookmarkHook(bookmark)) => {
                assert_eq!(book_or_rex, bookmark);
            }
            _ => assert!(false, "Unexpected err type"),
        };
    });
}

#[test]
fn test_load_hooks_bad_rust_hook() {
    async_unit::tokio_unit_test(|| {
        let mut config = default_repo_config();
        config.bookmarks = vec![BookmarkParams {
            bookmark: Bookmark::new("bm1").unwrap().into(),
            hooks: vec!["rust:hook1".into()],
            only_fast_forward: false,
            allowed_users: None,
        }];

        config.hooks = vec![HookParams {
            name: "rust:hook1".into(),
            code: Some("hook1 code".into()),
            hook_type: HookType::PerChangeset,
            config: Default::default(),
        }];

        let mut hm = hook_manager_blobrepo();

        match load_hooks(&mut hm, config)
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

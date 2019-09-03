// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This sub module contains a Lua implementation of hooks

#![deny(warnings)]

use super::errors::*;
use super::{
    phabricator_message_parser::PhabricatorMessage, ChangedFileType, Hook, HookChangeset,
    HookChangesetParents, HookContext, HookExecution, HookFile, HookRejectionInfo,
};
use aclchecker::Identity;
use cloned::cloned;
use context::CoreContext;
use failure::Error;
use futures::future::{ok, result};
use futures::{future, Future};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use hlua::{
    function0, function1, function2, AnyLuaString, AnyLuaValue, Lua, LuaError,
    LuaFunctionCallError, LuaTable, PushGuard, TuplePushError, Void,
};
use hlua_futures::{AnyFuture, LuaCoroutine, LuaCoroutineBuilder};
use lazy_static::lazy_static;
use linked_hash_map::LinkedHashMap;
use maplit::hashmap;
use metaconfig_types::HookConfig;
use mononoke_types::FileType;
use regex::{Regex, RegexBuilder};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

const HOOK_START_CODE_BASE: &str = include_str!("hook_start_base.lua");
const HOOK_START_CODE_CS: &str = include_str!("hook_start_cs.lua");
const HOOK_START_CODE_FILE: &str = include_str!("hook_start_file.lua");

#[derive(Clone, Debug)]
pub struct LuaHook {
    pub name: String,
    /// The Lua code of the hook
    pub code: String,
}

impl Hook<HookChangeset> for LuaHook {
    fn run(
        &self,
        ctx: CoreContext,
        context: HookContext<HookChangeset>,
    ) -> BoxFuture<HookExecution, Error> {
        let mut hook_info = hashmap! {
            "author" => context.data.author.to_string(),
            "comments" => context.data.comments.to_string(),
        };
        match context.data.parents {
            HookChangesetParents::None => (),
            HookChangesetParents::One(ref parent1_hash) => {
                hook_info.insert("parent1_hash", parent1_hash.to_string());
            }
            HookChangesetParents::Two(ref parent1_hash, ref parent2_hash) => {
                hook_info.insert("parent1_hash", parent1_hash.to_string());
                hook_info.insert("parent2_hash", parent2_hash.to_string());
            }
        }
        let mut code = HOOK_START_CODE_CS.to_string();
        code.push_str(HOOK_START_CODE_BASE);
        code.push_str(&self.code);

        let files_map: HashMap<String, HookFile> = context
            .data
            .files
            .iter()
            .map(|file| (file.path.clone(), file.clone()))
            .collect();
        let files_map2 = files_map.clone();

        let contains_string = {
            cloned!(ctx);
            move |path: String, string: String| -> Result<AnyFuture, Error> {
                match files_map.get(&path) {
                    Some(file) => {
                        let future = file
                            .contains_string(ctx.clone(), &string)
                            .map_err(|err| {
                                LuaError::ExecutionError(format!(
                                    "failed to get file content: {}",
                                    err
                                ))
                            })
                            .map(|contains| AnyLuaValue::LuaBoolean(contains));
                        Ok(AnyFuture::new(future))
                    }
                    None => Ok(AnyFuture::new(ok(AnyLuaValue::LuaBoolean(false)))),
                }
            }
        };
        let contains_string = function2(contains_string);
        let file_content = {
            cloned!(ctx, context);
            move |path: String| -> Result<AnyFuture, Error> {
                let future = context
                    .data
                    .file_content(ctx.clone(), path)
                    .map_err(|err| {
                        LuaError::ExecutionError(format!("failed to get file content: {}", err))
                    })
                    .map(|opt| match opt {
                        Some(content) => AnyLuaValue::LuaAnyString(AnyLuaString(content.to_vec())),
                        None => AnyLuaValue::LuaNil,
                    });
                Ok(AnyFuture::new(future))
            }
        };
        let file_content = function1(file_content);
        let file_len = {
            cloned!(ctx);
            move |path: String| -> Result<AnyFuture, Error> {
                match files_map2.get(&path) {
                    Some(file) => {
                        let future = file
                            .len(ctx.clone())
                            .map_err(|err| {
                                LuaError::ExecutionError(format!(
                                    "failed to get file content: {}",
                                    err
                                ))
                            })
                            .map(|len| AnyLuaValue::LuaNumber(len as f64));
                        Ok(AnyFuture::new(future))
                    }
                    None => Ok(AnyFuture::new(ok(AnyLuaValue::LuaBoolean(false)))),
                }
            }
        };
        let file_len = function1(file_len);

        let parse_commit_msg = {
            cloned!(context);
            move || -> Result<AnyFuture, Error> {
                let parsed_commit_msg = PhabricatorMessage::parse_message(&context.data.comments)
                    .to_lua()
                    .into_iter()
                    .map(|(key, val)| {
                        (
                            AnyLuaValue::LuaAnyString(AnyLuaString(key.as_bytes().to_vec())),
                            val,
                        )
                    })
                    .collect();
                Ok(AnyFuture::new(ok(AnyLuaValue::LuaArray(parsed_commit_msg))))
            }
        };
        let parse_commit_msg = function0(parse_commit_msg);

        let is_valid_reviewer = {
            let mocked_valid_reviewers = context
                .config
                .strings
                .get("test_mocked_valid_reviewers")
                .map(|mocked| mocked.split(",").map(String::from).collect::<HashSet<_>>());
            let reviewers_acl_checker = context.data.reviewers_acl_checker.clone();

            function1(move |user: String| -> Result<AnyFuture, Error> {
                if let Some(ref mocked) = mocked_valid_reviewers {
                    return Ok(AnyFuture::new(ok(AnyLuaValue::LuaBoolean(
                        mocked.contains(&user),
                    ))));
                }

                let regular_user = Identity::with_user(&user);
                let system_user = Identity::with_system_user(&user);
                let valid = match *reviewers_acl_checker {
                    Some(ref reviewers_acl_checker) => {
                        reviewers_acl_checker.is_member(&[&regular_user])
                            || reviewers_acl_checker.is_member(&[&system_user])
                    }
                    None => false,
                };
                Ok(AnyFuture::new(ok(AnyLuaValue::LuaBoolean(valid))))
            })
        };

        let mut lua = Lua::new();
        lua.openlibs();
        add_configs_lua(&mut lua, context.clone());
        add_regex_match_lua(&mut lua);
        lua.set("g__contains_string", contains_string);
        lua.set("g__file_len", file_len);
        lua.set("g__file_content", file_content);
        lua.set("g__parse_commit_msg", parse_commit_msg);
        lua.set("g__is_valid_reviewer", is_valid_reviewer);
        let res: Result<(), Error> = lua
            .execute::<()>(&code)
            .map_err(|e| ErrorKind::HookParseError(e.to_string()).into());
        if let Err(e) = res {
            return future::err(e).boxify();
        }
        // Note the lifetime becomes static as the into_get method moves the lua
        // and the later create moves it again into the coroutine
        let res: Result<LuaCoroutineBuilder<PushGuard<Lua<'static>>>, Error> = lua
            .into_get("g__hook_start")
            .map_err(|_| panic!("No g__hook_start"));
        let builder = match res {
            Ok(builder) => builder,
            Err(e) => return future::err(e).boxify(),
        };

        let mut files = vec![];

        for f in context.data.files {
            let ty = match f.ty {
                ChangedFileType::Added => "added",
                ChangedFileType::Deleted => "deleted",
                ChangedFileType::Modified => "modified",
            };
            files.push(hashmap! {
                "path" => f.path,
                "type" => ty.to_string(),
            });
        }

        self.convert_coroutine_res(builder.create((hook_info, files)))
    }
}

impl Hook<HookFile> for LuaHook {
    fn run(
        &self,
        ctx: CoreContext,
        context: HookContext<HookFile>,
    ) -> BoxFuture<HookExecution, Error> {
        let mut code = HOOK_START_CODE_FILE.to_string();
        code.push_str(HOOK_START_CODE_BASE);
        code.push_str(&self.code);
        let contains_string = {
            cloned!(ctx, context);
            move |string: String| -> Result<AnyFuture, Error> {
                let future = context
                    .data
                    .contains_string(ctx.clone(), &string)
                    .map_err(|err| {
                        LuaError::ExecutionError(format!("failed to get file content: {}", err))
                    })
                    .map(|contains| AnyLuaValue::LuaBoolean(contains));
                Ok(AnyFuture::new(future))
            }
        };
        let contains_string = function1(contains_string);
        let file_content = {
            cloned!(ctx, context);
            move || -> Result<AnyFuture, Error> {
                let future = context
                    .data
                    .file_content(ctx.clone())
                    .map_err(|err| {
                        LuaError::ExecutionError(format!("failed to get file content: {}", err))
                    })
                    .map(|content| AnyLuaValue::LuaAnyString(AnyLuaString(content.to_vec())));
                Ok(AnyFuture::new(future))
            }
        };
        let file_content = function0(file_content);
        let is_symlink = {
            cloned!(ctx, context);
            move || -> Result<AnyFuture, Error> {
                let future = context
                    .data
                    .file_type(ctx.clone())
                    .map_err(|err| {
                        LuaError::ExecutionError(format!("failed to get file content: {}", err))
                    })
                    .map(|file_type| {
                        let is_symlink = match file_type {
                            FileType::Symlink => true,
                            _ => false,
                        };
                        AnyLuaValue::LuaBoolean(is_symlink)
                    });
                Ok(AnyFuture::new(future))
            }
        };
        let is_symlink = function0(is_symlink);
        let file_len = {
            cloned!(ctx, context);
            move || -> Result<AnyFuture, Error> {
                let future = context
                    .data
                    .len(ctx.clone())
                    .map_err(|err| {
                        LuaError::ExecutionError(format!("failed to get file content: {}", err))
                    })
                    .map(|len| AnyLuaValue::LuaNumber(len as f64));
                Ok(AnyFuture::new(future))
            }
        };
        let file_len = function0(file_len);

        let mut lua = Lua::new();
        lua.openlibs();
        add_configs_lua(&mut lua, context.clone());
        add_regex_match_lua(&mut lua);
        lua.set("g__contains_string", contains_string);
        lua.set("g__file_len", file_len);
        lua.set("g__file_content", file_content);
        lua.set("g__is_symlink", is_symlink);
        let res: Result<(), Error> = lua
            .execute::<()>(&code)
            .map_err(|e| ErrorKind::HookParseError(e.to_string()).into());
        if let Err(e) = res {
            return future::err(e).boxify();
        }
        // Note the lifetime becomes static as the into_get method moves the lua
        // and the later create moves it again into the coroutine
        let res: Result<LuaCoroutineBuilder<PushGuard<Lua<'static>>>, Error> = lua
            .into_get("g__hook_start")
            .map_err(|_| panic!("No g__hook_start"));
        let builder = match res {
            Ok(builder) => builder,
            Err(e) => return future::err(e).boxify(),
        };
        let ty = match context.data.ty {
            ChangedFileType::Added => "added".to_string(),
            ChangedFileType::Deleted => "deleted".to_string(),
            ChangedFileType::Modified => "modified".to_string(),
        };
        let data = hashmap! {
            "path" => context.data.path.clone(),
            "type" => ty,
        };
        self.convert_coroutine_res(builder.create((HashMap::<&str, String, _>::new(), data)))
    }
}

impl LuaHook {
    pub fn new(name: String, code: String) -> LuaHook {
        LuaHook { name, code }
    }

    fn convert_coroutine_res(
        &self,
        res: Result<
            LuaCoroutine<PushGuard<Lua<'static>>, LuaTable<PushGuard<Lua<'static>>>>,
            LuaFunctionCallError<TuplePushError<Void, Void>>,
        >,
    ) -> BoxFuture<HookExecution, Error> {
        let res = res.map_err(|err| ErrorKind::HookRuntimeError(format!("{:#?}", err)));
        try_boxfuture!(res)
            .map_err(move |err| Error::from(ErrorKind::HookRuntimeError(format!("{:#?}", err))))
            .map(|mut t| {
                t.get::<bool, _, _>(1)
                    .ok_or(ErrorKind::HookRuntimeError("No hook return".to_string()).into())
                    .and_then(|acc| {
                        if acc {
                            Ok(HookExecution::Accepted)
                        } else {
                            let desc = t.get::<String, _, _>(2).ok_or(Error::from(
                                ErrorKind::HookRuntimeError("No description".to_string()),
                            ))?;
                            let long_desc = t.get::<String, _, _>(3);
                            Ok(HookExecution::Rejected(HookRejectionInfo::new_opt(
                                desc, long_desc,
                            )))
                        }
                    })
            })
            .flatten()
            .boxify()
    }
}

fn add_configs_lua<T: Clone>(lua: &mut Lua, context: HookContext<T>) {
    let HookConfig {
        strings,
        ints,
        bypass: _,
    } = context.config;
    lua.set("g__config_strings", strings);
    lua.set("g__config_ints", ints);
}

fn add_regex_match_lua(lua: &mut Lua) {
    lua.set(
        "g__regex_match",
        function2(
            |pattern: String, string: String| -> Result<AnyFuture, Error> {
                let future = cached_regex_match(pattern, string)
                    .map_err(|err| LuaError::ExecutionError(format!("invalid regex: {}", err)))
                    .map(|matched| AnyLuaValue::LuaBoolean(matched));

                Ok(AnyFuture::new(future))
            },
        ),
    )
}

fn cached_regex_match(
    pattern: String,
    string: String,
) -> impl Future<Item = bool, Error = regex::Error> {
    const REGEX_SIZE_LIMIT: usize = 10 * 1024;
    const REGEX_CACHE_SIZE: usize = 128;

    lazy_static! {
        static ref HOOK_REGEX_CACHE: Arc<RwLock<LinkedHashMap<String, Regex>>> =
            Arc::new(RwLock::new(LinkedHashMap::with_capacity(REGEX_CACHE_SIZE)));
    }

    let future = if let Some(r) = HOOK_REGEX_CACHE.read().unwrap().get(&pattern) {
        ok(r.is_match(&string)).left_future()
    } else {
        result(
            RegexBuilder::new(&pattern)
                .size_limit(REGEX_SIZE_LIMIT)
                .build(),
        )
        .and_then(move |r| {
            if HOOK_REGEX_CACHE.read().unwrap().len() > REGEX_CACHE_SIZE {
                HOOK_REGEX_CACHE.write().unwrap().pop_front();
            }
            HOOK_REGEX_CACHE.write().unwrap().insert(pattern, r.clone());
            ok(r.is_match(&string))
        })
        .right_future()
    };

    future
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        facebook, ChangedFileType, HookChangeset, HookChangesetParents, InMemoryFileContentStore,
    };
    use aclchecker::AclChecker;
    use assert_matches::assert_matches;
    use async_unit;
    use bookmarks::BookmarkName;
    use bytes::Bytes;
    use failure_ext::err_downcast;
    use futures::Future;
    use mercurial_types::{HgChangesetId, MPath};
    use std::str::FromStr;
    use std::sync::Arc;

    #[test]
    fn test_cs_hook_simple_rejected() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return false, 'fail'\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Rejected(_))
            );
        });
    }

    #[test]
    fn test_cs_hook_simple_fails_on_deleted_read() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 for _, file in ipairs(ctx.files) do\n\
                 if file.path == \"deleted\" then\n\
                 file:file_content()\n\
                 end\n\
                 end\n\
                 return true\n\
                 end",
            );
            assert!(run_changeset_hook(ctx.clone(), code, changeset).is_err());
        });
    }

    #[test]
    fn test_cs_hook_reviewers() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 local reviewers = ctx.parse_commit_msg()['reviewers']\n\
                 return not reviewers\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );

            let cs_id =
                HgChangesetId::from_str("473b2e715e0df6b2316010908879a3c78e275dd9").unwrap();
            let content_store = InMemoryFileContentStore::new();
            let reviewers_acl_checker = acl_checker();
            let hcs = HookChangeset::new(
                "some-author".into(),
                vec![],
                "blah blah blah\nReviewed By: user1, user2".into(),
                HookChangesetParents::One("p1-hash".into()),
                cs_id,
                Arc::new(content_store),
                reviewers_acl_checker,
            );
            let code = String::from(
                "hook = function (ctx)\n\
                 local reviewers = ctx.parse_commit_msg()['reviewed by']\n\
                 return #reviewers == 2\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, hcs),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_test_plan() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 local test_plan = ctx.parse_commit_msg()['test plan']\n\
                 return not test_plan\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );

            let cs_id =
                HgChangesetId::from_str("473b2e715e0df6b2316010908879a3c78e275dd9").unwrap();
            let content_store = InMemoryFileContentStore::new();
            let reviewers_acl_checker = acl_checker();
            let hcs = HookChangeset::new(
                "some-author".into(),
                vec![],
                "blah blah blah\nTest Plan: testinprod".into(),
                HookChangesetParents::One("p1-hash".into()),
                cs_id,
                Arc::new(content_store),
                reviewers_acl_checker,
            );
            let code = String::from(
                "hook = function (ctx)\n\
                 local test_plan = ctx.parse_commit_msg()['test plan']\n\
                 return test_plan == 'testinprod'\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, hcs),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_author_unixname() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.info.author_unixname == 'some-author'\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );

            let cs_id =
                HgChangesetId::from_str("473b2e715e0df6b2316010908879a3c78e275dd9").unwrap();
            let content_store = InMemoryFileContentStore::new();
            let reviewers_acl_checker = acl_checker();
            let hcs = HookChangeset::new(
                "Stanislau Hlebik <stash@fb.com>".into(),
                vec![],
                "blah blah blah\nTest Plan: testinprod".into(),
                HookChangesetParents::One("p1-hash".into()),
                cs_id,
                Arc::new(content_store),
                reviewers_acl_checker,
            );
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.info.author_unixname == 'stash'\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, hcs),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_not_valid_reviewer() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return not ctx.is_valid_reviewer('uyqdyqduygqwduygqwuydgqdfgbducbe2ubjweuhqwudh37')\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_valid_reviewer() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.is_valid_reviewer('zuck')\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_valid_reviewer_other() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.is_valid_reviewer('svcscm')\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_rejected_short_and_long_desc() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return false, \"emus\", \"ostriches\"\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Rejected(HookRejectionInfo{ref description,
                    ref long_description}))
                    if description==&"emus" && long_description==&"ostriches"
            );
        });
    }

    #[test]
    fn test_cs_hook_author() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.info.author == \"some-author\"\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_file_paths() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            // Arrays passed from rust -> lua appear to be 1 indexed in Lua land
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.files[0] == nil and ctx.files[1].path == \"file1\" and\n\
                 ctx.files[2].path == \"file2\" and ctx.files[3].path == \"file3\" and\n\
                 ctx.files[6] == nil\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_file_contains_string_match() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.files[1].contains_string(\"file1sausages\") and\n
                 ctx.files[2].contains_string(\"file2sausages\") and\n
                 ctx.files[3].contains_string(\"file3sausages\")\n
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_path_regex_match() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.files[1].path_regex_match(\"file[0-9]\") and\n
                 ctx.files[2].path_regex_match(\"f*2\") and\n
                 ctx.files[3].path_regex_match(\"fil.3\")\n
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_regex_match() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.regex_match(\"file[0-9]\", ctx.files[1].path) and\n
                 ctx.regex_match(\"f*2\", ctx.files[2].path) and\n
                 ctx.regex_match(\"fil.3\", ctx.files[3].path)\n
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_file_content_match() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.files[1].content() == \"file1sausages\" and\n
                 ctx.files[2].content() == \"file2sausages\" and\n
                 ctx.files[3].content() == \"file3sausages\" and\n
                 ctx.files[5].content() == \"modifiedsausages\"\n
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_other_file_content_match() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file_content(\"file1\") == \"file1sausages\" and\n
                 ctx.file_content(\"file2\") == \"file2sausages\" and\n
                 ctx.file_content(\"file3\") == \"file3sausages\"\n
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_file_content_not_found_returns_nil() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file_content(\"no/such/path\") == nil\n
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_check_type() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 local added_file = ctx.files[1]
                 local added = added_file.is_added() and \
                    not added_file.is_deleted() and not added_file.is_modified()

                 local deleted_file = ctx.files[4]
                 local deleted = not deleted_file.is_added() and \
                    deleted_file.is_deleted() and not deleted_file.is_modified()

                 local modified_file = ctx.files[5]
                 local modified = not modified_file.is_added() and \
                    not modified_file.is_deleted() and modified_file.is_modified()

                 return added and deleted and modified
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_deleted() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 for _, f in ipairs(ctx.files) do
                    if f.is_deleted() then
                        return f.path == \"deleted\"\n
                    end
                 end
                 return false
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_file_len() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.files[1].len() == 13 and\n
                 ctx.files[2].len() == 13 and\n
                 ctx.files[3].len() == 13 and\n
                 ctx.files[5].len() == 16\n
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_comments() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.info.comments == \"some-comments\"\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_one_parent() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.info.parent1_hash == \"p1-hash\" and \n\
                 ctx.info.parent2_hash == nil\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_two_parents() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let mut changeset = default_changeset();
            changeset.parents = HookChangesetParents::Two("p1-hash".into(), "p2-hash".into());
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.info.parent1_hash == \"p1-hash\" and \n\
                 ctx.info.parent2_hash == \"p2-hash\"\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_no_parents() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let mut changeset = default_changeset();
            changeset.parents = HookChangesetParents::None;
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.info.parent1_hash == nil and \n\
                 ctx.info.parent2_hash == nil\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_no_hook_func() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "elephants = function (ctx)\n\
                 return true\n\
                 end",
            );
            assert_matches!(
               err_downcast!(run_changeset_hook(ctx.clone(), code, changeset).unwrap_err(), err: ErrorKind => err),
               Ok(ErrorKind::HookRuntimeError(ref msg)) if msg.contains("no hook function")
            );
        });
    }

    #[test]
    fn test_cs_hook_invalid_hook() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from("invalid code");
            assert_matches!(
               err_downcast!(run_changeset_hook(ctx.clone(), code, changeset).unwrap_err(), err: ErrorKind => err),
               Ok(ErrorKind::HookParseError(ref err_msg))
                   if err_msg.starts_with("Syntax error:")
            );
        });
    }

    #[test]
    fn test_cs_hook_exception() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 if ctx.info.author == \"some-author\" then\n\
                 error(\"fubar\")\n\
                 end\n\
                 return true\n\
                 end",
            );
            assert_matches!(
               err_downcast!(run_changeset_hook(ctx.clone(), code, changeset).unwrap_err(), err: ErrorKind => err),
               Ok(ErrorKind::HookRuntimeError(ref err_msg))
                   if err_msg.starts_with("LuaError")
            );
        });
    }

    #[test]
    fn test_cs_hook_invalid_return_val() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return \"aardvarks\"\n\
                 end",
            );
            assert_matches!(
               err_downcast!(run_changeset_hook(ctx.clone(), code, changeset).unwrap_err(), err: ErrorKind => err),
               Ok(ErrorKind::HookRuntimeError(ref err_msg))
                   if err_msg.contains("invalid hook return type")
            );
        });
    }

    #[test]
    fn test_cs_hook_no_short_desc() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return false\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_changeset_hook(ctx.clone(), code, changeset).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg))
                    if err_msg.contains("No description")
            );
        });
    }

    #[test]
    fn test_cs_hook_invalid_short_desc() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return false, 23, \"long desc\"\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_changeset_hook(ctx.clone(), code, changeset).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg))
                    if err_msg.contains("invalid hook failure short description type")
            );
        });
    }

    #[test]
    fn test_cs_hook_invalid_long_desc() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return false, \"short desc\", 23\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_changeset_hook(ctx.clone(), code, changeset).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg))
                    if err_msg.contains("invalid hook failure long description type")
            );
        });
    }

    #[test]
    fn test_cs_hook_desc_when_hooks_is_accepted() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return true, \"short\", \"long\"\n\
                 end",
            );
            assert_matches!(
               err_downcast!(run_changeset_hook(ctx.clone(), code, changeset).unwrap_err(), err: ErrorKind => err),
               Ok(ErrorKind::HookRuntimeError(ref err_msg))
                   if err_msg.contains("failure description must only be set if hook fails")
            );
        });
    }

    #[test]
    fn test_cs_hook_long_desc_when_hooks_is_accepted() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return true, nil, \"long\"\n\
                 end",
            );
            assert_matches!(
               err_downcast!(run_changeset_hook(ctx.clone(), code, changeset).unwrap_err(), err: ErrorKind => err),
               Ok(ErrorKind::HookRuntimeError(ref err_msg))
                   if err_msg.contains("failure long description must only be set if hook fails")
            );
        });
    }

    #[test]
    fn test_cs_hook_no_io_nor_os() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return io == nil and os == nil\n
                 end",
            );
            assert_matches!(
                run_changeset_hook(ctx.clone(), code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_file_hook_path() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file.path == \"/a/b/c.txt\"\n\
                 end",
            );
            assert_matches!(
                run_file_hook(ctx.clone(), code, hook_file),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_file_hook_contains_string_matches() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file.contains_string(\"sausages\")\n\
                 end",
            );
            assert_matches!(
                run_file_hook(ctx.clone(), code, hook_file),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_file_hook_contains_string_no_matches() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file.contains_string(\"gerbils\"), 'fail'\n\
                 end",
            );
            assert_matches!(
                run_file_hook(ctx.clone(), code, hook_file),
                Ok(HookExecution::Rejected(_))
            );
        });
    }

    #[test]
    fn test_file_hook_path_regex_match_no_matches() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file.path_regex_match(\"a[0-9]bcde\"), 'fail'\n\
                 end",
            );
            assert_matches!(
                run_file_hook(ctx.clone(), code, hook_file),
                Ok(HookExecution::Rejected(_))
            );
        });
    }

    #[test]
    fn test_file_hook_regex_match_no_matches() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.regex_match(\"a[0-9]bcde\", ctx.file.path), 'fail'\n\
                 end",
            );
            assert_matches!(
                run_file_hook(ctx.clone(), code, hook_file),
                Ok(HookExecution::Rejected(_))
            );
        });
    }

    #[test]
    fn test_file_hook_path_regex_match_matches() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file.path_regex_match(\"a*.txt\")\n\
                 end",
            );
            assert_matches!(
                run_file_hook(ctx.clone(), code, hook_file),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_file_hook_regex_match_matches() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.regex_match(\"a*.txt\", ctx.file.path)\n\
                 end",
            );
            assert_matches!(
                run_file_hook(ctx.clone(), code, hook_file),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_file_hook_path_regex_match_invalid_regex() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file.path_regex_match(\"[0-\")\n\
                 end",
            );
            assert_matches!(
               err_downcast!(run_file_hook(ctx.clone(),code, hook_file).unwrap_err(), err: ErrorKind => err),
               Ok(ErrorKind::HookRuntimeError(ref err_msg))
                   if err_msg.contains("invalid regex")
            );
        });
    }

    #[test]
    fn test_file_hook_regex_match_invalid_regex() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.regex_match(\"[0-\", ctx.file.path)\n\
                 end",
            );
            assert_matches!(
               err_downcast!(run_file_hook(ctx.clone(),code, hook_file).unwrap_err(), err: ErrorKind => err),
               Ok(ErrorKind::HookRuntimeError(ref err_msg))
                   if err_msg.contains("invalid regex")
            );
        });
    }

    #[test]
    fn test_file_hook_content_matches() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file.content() == \"sausages\"\n\
                 end",
            );
            assert_matches!(
                run_file_hook(ctx.clone(), code, hook_file),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_file_hook_is_symlink() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_symlink_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file.is_symlink()\n\
                 end",
            );
            assert_matches!(
                run_file_hook(ctx.clone(), code, hook_file),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_file_hook_is_not_symlink() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file.is_symlink(), 'fail'\n\
                 end",
            );
            assert_matches!(
                run_file_hook(ctx.clone(), code, hook_file),
                Ok(HookExecution::Rejected(_))
            );
        });
    }

    #[test]
    fn test_file_hook_removed() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_removed_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file.path == \"/a/b/c.txt\" and ctx.file.is_deleted()\n\
                 end",
            );
            assert_matches!(
                run_file_hook(ctx.clone(), code, hook_file),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_file_hook_len_matches() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file.len() == 8\n\
                 end",
            );
            assert_matches!(
                run_file_hook(ctx.clone(), code, hook_file),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_file_hook_len_no_matches() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file.len() == 123, 'fail'\n\
                 end",
            );
            assert_matches!(
                run_file_hook(ctx.clone(), code, hook_file),
                Ok(HookExecution::Rejected(_))
            );
        });
    }

    #[test]
    fn test_file_hook_rejected() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return false, 'fail'\n\
                 end",
            );
            assert_matches!(
                run_file_hook(ctx.clone(), code, hook_file),
                Ok(HookExecution::Rejected(_))
            );
        });
    }

    #[test]
    fn test_file_hook_no_hook_func() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "elephants = function (ctx)\n\
                 return true\n\
                 end",
            );
            assert_matches!(
               err_downcast!(run_file_hook(ctx.clone(),code, hook_file).unwrap_err(), err: ErrorKind => err),
               Ok(ErrorKind::HookRuntimeError(ref err_msg)) if err_msg.contains("no hook function")
            );
        });
    }

    #[test]
    fn test_file_hook_invalid_hook() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from("invalid code");
            assert_matches!(
               err_downcast!(run_file_hook(ctx.clone(),code, hook_file).unwrap_err(), err: ErrorKind => err),
               Ok(ErrorKind::HookParseError(ref err_msg))
                   if err_msg.starts_with("Syntax error:")
            );
        });
    }

    #[test]
    fn test_file_hook_exception() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 if ctx.file.path == \"/a/b/c.txt\" then\n\
                 error(\"fubar\")\n\
                 end\n\
                 return true\n\
                 end",
            );
            assert_matches!(
               err_downcast!(run_file_hook(ctx.clone(),code, hook_file).unwrap_err(), err: ErrorKind => err),
               Ok(ErrorKind::HookRuntimeError(ref err_msg))
                   if err_msg.starts_with("LuaError")
            );
        });
    }

    #[test]
    fn test_file_hook_invalid_return_val() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return \"aardvarks\"\n\
                 end",
            );
            assert_matches!(
               err_downcast!(run_file_hook(ctx.clone(),code, hook_file).unwrap_err(), err: ErrorKind => err),
               Ok(ErrorKind::HookRuntimeError(ref err_msg))
                   if err_msg.contains("invalid hook return type")
            );
        });
    }

    #[test]
    fn test_file_hook_no_short_desc() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return false\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_file_hook(ctx.clone(),code, hook_file).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg))
                    if err_msg.contains("No description")
            );
        });
    }

    #[test]
    fn test_file_hook_invalid_short_desc() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return false, 23, \"long desc\"\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_file_hook(ctx.clone(),code, hook_file).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg))
                    if err_msg.contains("invalid hook failure short description type")
            );
        });
    }

    #[test]
    fn test_file_hook_invalid_long_desc() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return false, \"short desc\", 23\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_file_hook(ctx.clone(),code, hook_file).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg))
                    if err_msg.contains("invalid hook failure long description type")
            );
        });
    }

    #[test]
    fn test_file_hook_no_io_nor_os() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let hook_file = default_hook_added_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return io == nil and os == nil\n
                 end",
            );
            assert_matches!(
                run_file_hook(ctx.clone(), code, hook_file),
                Ok(HookExecution::Accepted)
            );
        });
    }

    const HOOK_CHECKING_CONFIGS: &str = r#"
hook = function (ctx)
    if ctx.config_strings["test"] ~= "val" then
        return false, "", "missing strings config"
    end

    if ctx.config_strings["test2"] ~= nil then
        return false, "", "existing strings config"
    end

    if ctx.config_ints["test"] ~= 44 then
        return false, "", "missing ints config"
    end

    if ctx.config_ints["test2"] ~= nil then
        return false, "", "existing ints config"
    end

    return true, nil, nil
end"#;

    fn run_test_for_config_reading<T: Clone>(
        ctx: CoreContext,
        context_construct: impl Fn(HookConfig) -> HookContext<T>,
    ) where
        LuaHook: Hook<T>,
    {
        let assert_rejected = |res, desc| {
            assert_matches!(
                res,
                Ok(HookExecution::Rejected(ref info))
                    if info.description == "" && info.long_description == desc
            );
        };

        let hook = LuaHook::new("testhook".into(), HOOK_CHECKING_CONFIGS.into());

        let context = context_construct(HookConfig {
            bypass: None,
            strings: hashmap! { "test".to_string() => "val".to_string() },
            ints: hashmap! { "test".to_string() => 44 },
        });
        assert_matches!(
            hook.run(ctx.clone(), context).wait(),
            Ok(HookExecution::Accepted)
        );

        let context = context_construct(HookConfig {
            bypass: None,
            strings: hashmap! {},
            ints: hashmap! {},
        });
        assert_rejected(
            hook.run(ctx.clone(), context).wait(),
            "missing strings config",
        );

        let context = context_construct(HookConfig {
            bypass: None,
            strings: hashmap! {
                "test".to_string() => "val".to_string(),
                "test2".to_string() => "val2".to_string(),
            },
            ints: hashmap! {},
        });
        assert_rejected(
            hook.run(ctx.clone(), context).wait(),
            "existing strings config",
        );

        let context = context_construct(HookConfig {
            bypass: None,
            strings: hashmap! { "test".to_string() => "val".to_string() },
            ints: hashmap! {},
        });
        assert_rejected(hook.run(ctx.clone(), context).wait(), "missing ints config");

        let context = context_construct(HookConfig {
            bypass: None,
            strings: hashmap! { "test".to_string() => "val".to_string() },
            ints: hashmap! {
                "test".to_string() => 44,
                "test2".to_string() => 44,
            },
        });
        assert_rejected(
            hook.run(ctx.clone(), context).wait(),
            "existing ints config",
        );
    }

    #[test]
    fn test_cs_hook_config_reading() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();

            run_test_for_config_reading(ctx, |conf| {
                HookContext::new(
                    "testhook".into(),
                    conf,
                    default_changeset(),
                    BookmarkName::new("book").unwrap(),
                )
            });
        });
    }

    #[test]
    fn test_file_hook_config_reading() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();

            run_test_for_config_reading(ctx, |conf| {
                HookContext::new(
                    "testhook".into(),
                    conf,
                    default_hook_added_file(),
                    BookmarkName::new("book").unwrap(),
                )
            });
        });
    }

    fn run_changeset_hook(
        ctx: CoreContext,
        code: String,
        changeset: HookChangeset,
    ) -> Result<HookExecution, Error> {
        let hook = LuaHook::new(String::from("testhook"), code.to_string());
        let context = HookContext::new(
            hook.name.clone(),
            Default::default(),
            changeset,
            BookmarkName::new("book").unwrap(),
        );
        hook.run(ctx, context).wait()
    }

    fn run_file_hook(
        ctx: CoreContext,
        code: String,
        hook_file: HookFile,
    ) -> Result<HookExecution, Error> {
        let hook = LuaHook::new(String::from("testhook"), code.to_string());
        let context = HookContext::new(
            hook.name.clone(),
            Default::default(),
            hook_file,
            BookmarkName::new("book").unwrap(),
        );
        hook.run(ctx, context).wait()
    }

    use mercurial_types::HgFileNodeId;
    use mercurial_types_mocks::nodehash::{
        FIVES_FNID, FOURS_FNID, ONES_FNID, THREES_FNID, TWOS_FNID,
    };

    fn default_changeset() -> HookChangeset {
        let added = vec![
            ("file1".into(), ONES_FNID),
            ("file2".into(), TWOS_FNID),
            ("file3".into(), THREES_FNID),
        ];
        let deleted = vec![("deleted".into(), FOURS_FNID)];
        let modified = vec![("modified".into(), FIVES_FNID)];
        create_hook_changeset(added, deleted, modified)
    }

    fn to_mpath(string: &str) -> MPath {
        // Please... avert your eyes
        MPath::new(string.to_string().as_bytes().to_vec()).unwrap()
    }

    fn create_hook_changeset(
        added: Vec<(String, HgFileNodeId)>,
        deleted: Vec<(String, HgFileNodeId)>,
        modified: Vec<(String, HgFileNodeId)>,
    ) -> HookChangeset {
        let mut content_store = InMemoryFileContentStore::new();
        let cs_id = HgChangesetId::from_str("473b2e715e0df6b2316010908879a3c78e275dd9").unwrap();
        for (path, entry_id) in added.iter().chain(modified.iter()) {
            let content = path.clone() + "sausages";
            let content_bytes: Bytes = content.into();
            content_store.insert(cs_id, to_mpath(path), *entry_id, content_bytes.into());
        }
        let content_store = Arc::new(content_store);
        let content_store2 = content_store.clone();

        let create_hook_files =
            move |files: Vec<(String, HgFileNodeId)>, ty: ChangedFileType| -> Vec<HookFile> {
                files
                    .into_iter()
                    .map(|(path, hash)| {
                        HookFile::new(
                            path.clone(),
                            content_store.clone(),
                            cs_id,
                            ty.clone(),
                            Some((hash, FileType::Regular)),
                        )
                    })
                    .collect()
            };

        let mut hook_files = vec![];
        hook_files.extend(create_hook_files(added, ChangedFileType::Added));
        hook_files.extend(create_hook_files(deleted, ChangedFileType::Deleted));
        hook_files.extend(create_hook_files(modified, ChangedFileType::Modified));
        let reviewers_acl_checker = acl_checker();
        HookChangeset::new(
            "some-author".into(),
            hook_files,
            "some-comments".into(),
            HookChangesetParents::One("p1-hash".into()),
            cs_id,
            content_store2,
            reviewers_acl_checker,
        )
    }

    fn default_hook_symlink_file() -> HookFile {
        let mut content_store = InMemoryFileContentStore::new();
        let cs_id = HgChangesetId::from_str("473b2e715e0df6b2316010908879a3c78e275dd9").unwrap();
        let path = "/a/b/c.txt";
        content_store.insert(cs_id.clone(), to_mpath(path), ONES_FNID, "sausages".into());
        HookFile::new(
            path.into(),
            Arc::new(content_store),
            cs_id,
            ChangedFileType::Added,
            Some((ONES_FNID, FileType::Symlink)),
        )
    }

    fn default_hook_added_file() -> HookFile {
        let mut content_store = InMemoryFileContentStore::new();
        let cs_id = HgChangesetId::from_str("473b2e715e0df6b2316010908879a3c78e275dd9").unwrap();
        let path = "/a/b/c.txt";
        content_store.insert(cs_id.clone(), to_mpath(path), ONES_FNID, "sausages".into());
        HookFile::new(
            path.into(),
            Arc::new(content_store),
            cs_id,
            ChangedFileType::Added,
            Some((ONES_FNID, FileType::Regular)),
        )
    }

    fn default_hook_removed_file() -> HookFile {
        let content_store = InMemoryFileContentStore::new();
        let cs_id = HgChangesetId::from_str("473b2e715e0df6b2316010908879a3c78e275dd9").unwrap();
        HookFile::new(
            "/a/b/c.txt".into(),
            Arc::new(content_store),
            cs_id,
            ChangedFileType::Deleted,
            None,
        )
    }

    fn acl_checker() -> Arc<Option<AclChecker>> {
        let checker = AclChecker::new(&Identity::from_groupname(
            facebook::REVIEWERS_ACL_GROUP_NAME,
        ))
        .expect("couldnt get acl checker");
        assert!(checker.do_wait_updated(10000));
        Arc::new(Some(checker))
    }

}

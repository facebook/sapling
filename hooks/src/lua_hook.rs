// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This sub module contains a Lua implementation of hooks

#![deny(warnings)]

use super::{Hook, HookChangeset, HookChangesetParents, HookContext, HookExecution, HookFile,
            HookRejectionInfo};
use failure::Error;
use futures::{failed, Future};
use futures_ext::{BoxFuture, FutureExt};
use hlua::{Lua, LuaFunctionCallError, PushGuard, TuplePushError, Void};
use hlua_futures::{LuaCoroutine, LuaCoroutineBuilder};

#[derive(Clone)]
pub struct LuaHook {
    pub name: String,
    /// The Lua code of the hook
    pub code: String,
}

impl Hook<HookChangeset> for LuaHook {
    fn run(&self, context: HookContext<HookChangeset>) -> BoxFuture<HookExecution, Error> {
        let hook = (*self).clone();
        let hook_name = hook.name.clone();
        let mut hook_info = hashmap! {
            "repo_name" => context.repo_name.to_string(),
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
        let builder = match self.create_builder(hook.clone()) {
            Ok(builder) => builder,
            Err(e) => return failed(e).boxify(),
        };
        self.convert_cr_res(
            builder.create((hook_info, context.data.files.clone())),
            hook_name,
        )
    }
}

impl Hook<HookFile> for LuaHook {
    fn run(&self, context: HookContext<HookFile>) -> BoxFuture<HookExecution, Error> {
        let hook = (*self).clone();
        let hook_name = hook.name.clone();
        let hook_info = hashmap! {
            "repo_name" => context.repo_name.to_string(),
        };
        let builder = match self.create_builder(hook) {
            Ok(builder) => builder,
            Err(e) => return failed(e).boxify(),
        };
        self.convert_cr_res(
            builder.create((hook_info, context.data.path.clone())),
            hook_name,
        )
    }
}

impl LuaHook {
    pub fn new(name: String, code: String) -> LuaHook {
        LuaHook { name, code }
    }

    fn convert_cr_res(
        &self,
        res: Result<
            LuaCoroutine<PushGuard<Lua<'static>>, bool>,
            LuaFunctionCallError<TuplePushError<Void, Void>>,
        >,
        hook_name: String,
    ) -> BoxFuture<HookExecution, Error> {
        let res = res.map_err(|err| {
            ErrorKind::HookRuntimeError(hook_name.clone().into(), format!("{:?}", err)).into()
        });
        match res {
            Ok(cr) => {
                cr.map(|b| {
                    if b {
                        HookExecution::Accepted
                    } else {
                        // TODO allow proper hook rejection to be set from Lua hook
                        HookExecution::Rejected(HookRejectionInfo::new(
                            "short desc".into(),
                            "long desc".into(),
                        ))
                    }
                }).map_err(move |err| {
                        ErrorKind::HookRuntimeError(hook_name.into(), format!("{:?}", err)).into()
                    })
                    .boxify()
            }

            Err(e) => Box::new(failed(e)),
        }
    }

    fn create_builder(
        &self,
        hook: LuaHook,
    ) -> Result<LuaCoroutineBuilder<PushGuard<Lua<'static>>>, Error> {
        let mut lua = Lua::new();
        lua.openlibs();
        let res: Result<(), Error> = lua.execute::<()>(&hook.code)
            .map_err(|e| ErrorKind::HookParseError(hook.name.clone().into(), e.to_string()).into());
        res?;
        // Note the lifetime becomes static as the into_get method moves the lua
        // and the later create moves it again into the coroutine
        lua.into_get("hook")
            .map_err(|_| ErrorKind::NoHookFunctionError(hook.name.clone().into()).into())
    }
}

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "No hook function found for hook '{}'", _0)] NoHookFunctionError(String),
    #[fail(display = "Error while parsing hook '{}': {}", _0, _1)] HookParseError(String, String),
    #[fail(display = "Error while running hook '{}': {}", _0, _1)] HookRuntimeError(String, String),
}

#[cfg(test)]
mod test {
    use super::*;
    use super::super::{HookChangeset, HookChangesetParents};
    use async_unit;
    use futures::Future;

    #[test]
    fn test_cs_hook_rejected() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (info, files)\n\
                 return info.author == \"mr blobby\"\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(code, changeset),
                Ok(HookExecution::Rejected(_))
            );
        });
    }

    #[test]
    fn test_cs_hook_author() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (info, files)\n\
                 return info.author == \"some-author\"\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_files() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            // Arrays passed from rust -> lua appear to be 1 indexed in Lua land
            let code = String::from(
                "hook = function (info, files)\n\
                 return files[0] == nil and files[1] == \"file1\" and\n\
                 files[2] == \"file2\" and files[3] == \"file3\" and\n\
                 files[4] == nil\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_comments() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (info, files)\n\
                 return info.comments == \"some-comments\"\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_repo_name() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (info, files)\n\
                 return info.repo_name == \"some-repo\"\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_one_parent() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (info, files)\n\
                 return info.parent1_hash == \"p1-hash\" and \n\
                 info.parent2_hash == nil\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_two_parents() {
        async_unit::tokio_unit_test(|| {
            let mut changeset = default_changeset();
            changeset.parents = HookChangesetParents::Two("p1-hash".into(), "p2-hash".into());
            let code = String::from(
                "hook = function (info, files)\n\
                 return info.parent1_hash == \"p1-hash\" and \n\
                 info.parent2_hash == \"p2-hash\"\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_no_parents() {
        async_unit::tokio_unit_test(|| {
            let mut changeset = default_changeset();
            changeset.parents = HookChangesetParents::None;
            let code = String::from(
                "hook = function (info, files)\n\
                 return info.parent1_hash == nil and \n\
                 info.parent2_hash == nil\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(code, changeset),
                Ok(HookExecution::Accepted)
            );
        });
    }

    #[test]
    fn test_cs_hook_no_hook_func() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "elephants = function (info, files)\n\
                 return true\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(code, changeset).unwrap_err().downcast::<ErrorKind>(),
                Ok(ErrorKind::NoHookFunctionError(ref hook_name)) if hook_name == "testhook"
             );
        });
    }

    #[test]
    fn test_cs_hook_invalid_hook() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from("invalid code");
            assert_matches!(
                run_changeset_hook(code, changeset).unwrap_err().downcast::<ErrorKind>(),
                Ok(ErrorKind::HookParseError(ref hook_name, ref err_msg))
                    if hook_name == "testhook" && err_msg.starts_with("Syntax error:")
             );
        });
    }

    #[test]
    fn test_cs_hook_exception() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (info, files)\n\
                 if info.author == \"some-author\" then\n\
                 error(\"fubar\")\n\
                 end\n\
                 return true\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(code, changeset).unwrap_err().downcast::<ErrorKind>(),
                Ok(ErrorKind::HookRuntimeError(ref hook_name, ref err_msg))
                    if hook_name == "testhook" && err_msg.starts_with("LuaError")
             );
        });
    }

    #[test]
    fn test_cs_hook_invalid_return_val() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (info, files)\n\
                 return \"aardvarks\"\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(code, changeset).unwrap_err().downcast::<ErrorKind>(),
                Ok(ErrorKind::HookRuntimeError(ref hook_name, ref err_msg))
                    if hook_name == "testhook" && err_msg.starts_with("LuaError")
             );
        });
    }

    #[test]
    fn test_file_hook_path() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from(
                "hook = function (info, file)\n\
                 return file == \"/a/b/c.txt\"\n\
                 end",
            );
            assert_matches!(run_file_hook(code, hook_file), Ok(HookExecution::Accepted));
        });
    }

    #[test]
    fn test_file_hook_repo_name() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from(
                "hook = function (info, file)\n\
                 return info.repo_name == \"some-repo\"\n\
                 end",
            );
            assert_matches!(run_file_hook(code, hook_file), Ok(HookExecution::Accepted));
        });
    }

    #[test]
    fn test_file_hook_rejected() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from(
                "hook = function (info, file)\n\
                 return false\n\
                 end",
            );
            assert_matches!(
                run_file_hook(code, hook_file),
                Ok(HookExecution::Rejected(_))
            );
        });
    }

    #[test]
    fn test_file_hook_no_hook_func() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from(
                "elephants = function (info, file)\n\
                 return true\n\
                 end",
            );
            assert_matches!(
                run_file_hook(code, hook_file).unwrap_err().downcast::<ErrorKind>(),
                Ok(ErrorKind::NoHookFunctionError(ref hook_name)) if hook_name == "testhook"
             );
        });
    }

    #[test]
    fn test_file_hook_invalid_hook() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from("invalid code");
            assert_matches!(
                run_file_hook(code, hook_file).unwrap_err().downcast::<ErrorKind>(),
                Ok(ErrorKind::HookParseError(ref hook_name, ref err_msg))
                    if hook_name == "testhook" && err_msg.starts_with("Syntax error:")
             );
        });
    }

    #[test]
    fn test_file_hook_exception() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from(
                "hook = function (info, file)\n\
                 if file == \"/a/b/c.txt\" then\n\
                 error(\"fubar\")\n\
                 end\n\
                 return true\n\
                 end",
            );
            assert_matches!(
                run_file_hook(code, hook_file).unwrap_err().downcast::<ErrorKind>(),
                Ok(ErrorKind::HookRuntimeError(ref hook_name, ref err_msg))
                    if hook_name == "testhook" && err_msg.starts_with("LuaError")
             );
        });
    }

    #[test]
    fn test_file_hook_invalid_return_val() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from(
                "hook = function (info, file)\n\
                 return \"aardvarks\"\n\
                 end",
            );
            assert_matches!(
                run_file_hook(code, hook_file).unwrap_err().downcast::<ErrorKind>(),
                Ok(ErrorKind::HookRuntimeError(ref hook_name, ref err_msg))
                    if hook_name == "testhook" && err_msg.starts_with("LuaError")
             );
        });
    }

    fn run_changeset_hook(code: String, changeset: HookChangeset) -> Result<HookExecution, Error> {
        let hook = LuaHook::new(String::from("testhook"), code.to_string());
        let context = HookContext::new(hook.name.clone(), "some-repo".into(), changeset);
        hook.run(context).wait()
    }

    fn run_file_hook(code: String, hook_file: HookFile) -> Result<HookExecution, Error> {
        let hook = LuaHook::new(String::from("testhook"), code.to_string());
        let context = HookContext::new(hook.name.clone(), "some-repo".into(), hook_file);
        hook.run(context).wait()
    }

    fn default_changeset() -> HookChangeset {
        let files = vec!["file1".into(), "file2".into(), "file3".into()];
        HookChangeset::new(
            "some-author".into(),
            files,
            "some-comments".into(),
            HookChangesetParents::One("p1-hash".into()),
        )
    }

    fn default_hook_file() -> HookFile {
        HookFile::new("/a/b/c.txt".into())
    }
}

// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This sub module contains a Lua implementation of hooks

#![deny(warnings)]

use super::{Hook, HookChangeset, HookChangesetParents, HookContext, HookExecution, HookFile,
            HookRejectionInfo};
use super::errors::*;
use failure::Error;
use futures::{failed, Future};
use futures_ext::{BoxFuture, FutureExt};
use hlua::{AnyLuaValue, Lua, LuaError, LuaFunctionCallError, LuaTable, PushGuard, TuplePushError,
           Void, function0, function1};
use hlua_futures::{AnyFuture, LuaCoroutine, LuaCoroutineBuilder};

const HOOK_START_CODE_BASE: &str = include_str!("hook_start_base.lua");

const HOOK_START_CODE_CS: &str = "
__hook_start = function(info, arg)
    return __hook_start_base(info, arg, function(arg, ctx)
        ctx.files=arg
    end)
end
";

const HOOK_START_CODE_FILE: &str = "
__hook_start = function(info, arg)
    return __hook_start_base(info, arg, function(arg, ctx)
        local file = {}
        file.path = arg
        file.contains_string = function(s) return coroutine.yield(__contains_string(s)) end
        file.len = function() return coroutine.yield(__file_len()) end
        ctx.file = file
    end)
end
";

#[derive(Clone)]
pub struct LuaHook {
    pub name: String,
    /// The Lua code of the hook
    pub code: String,
}

impl Hook<HookChangeset> for LuaHook {
    fn run(&self, context: HookContext<HookChangeset>) -> BoxFuture<HookExecution, Error> {
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
        let mut code = HOOK_START_CODE_CS.to_string();
        code.push_str(HOOK_START_CODE_BASE);
        code.push_str(&self.code);
        let mut lua = Lua::new();
        // Note that we explicitly open libraries not use lua.openlibs as we do not
        // want to load some packages e.g. io and os.
        lua.open_base();
        lua.open_bit32();
        lua.open_coroutine();
        lua.open_math();
        lua.open_package();
        lua.open_string();
        lua.open_table();
        let res: Result<(), Error> = lua.execute::<()>(&code)
            .map_err(|e| ErrorKind::HookParseError(e.to_string()).into());
        if let Err(e) = res {
            return failed(e).boxify();
        }
        // Note the lifetime becomes static as the into_get method moves the lua
        // and the later create moves it again into the coroutine
        let res: Result<LuaCoroutineBuilder<PushGuard<Lua<'static>>>, Error> = lua.into_get(
            "__hook_start",
        ).map_err(|_| panic!("No __hook_start"));
        let builder = match res {
            Ok(builder) => builder,
            Err(e) => return failed(e).boxify(),
        };
        self.convert_coroutine_res(builder.create((hook_info, context.data.files.clone())))
    }
}

impl Hook<HookFile> for LuaHook {
    fn run(&self, context: HookContext<HookFile>) -> BoxFuture<HookExecution, Error> {
        let hook_info = hashmap! {
            "repo_name" => context.repo_name.to_string(),
        };
        let mut code = HOOK_START_CODE_FILE.to_string();
        code.push_str(HOOK_START_CODE_BASE);
        code.push_str(&self.code);
        let contains_string = {
            cloned!(context);
            move |string: String| -> Result<AnyFuture, Error> {
                let future = context
                    .data
                    .contains_string(&string)
                    .map_err(|err| {
                        LuaError::ExecutionError(format!("failed to get file content: {}", err))
                    })
                    .map(|contains| AnyLuaValue::LuaBoolean(contains));
                Ok(AnyFuture::new(future))
            }
        };
        let contains_string = function1(contains_string);
        let file_len = {
            cloned!(context);
            move || -> Result<AnyFuture, Error> {
                let future = context
                    .data
                    .len()
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
        lua.set("__contains_string", contains_string);
        lua.set("__file_len", file_len);
        let res: Result<(), Error> = lua.execute::<()>(&code)
            .map_err(|e| ErrorKind::HookParseError(e.to_string()).into());
        if let Err(e) = res {
            return failed(e).boxify();
        }
        // Note the lifetime becomes static as the into_get method moves the lua
        // and the later create moves it again into the coroutine
        let res: Result<LuaCoroutineBuilder<PushGuard<Lua<'static>>>, Error> = lua.into_get(
            "__hook_start",
        ).map_err(|_| panic!("No __hook_start"));
        let builder = match res {
            Ok(builder) => builder,
            Err(e) => return failed(e).boxify(),
        };
        self.convert_coroutine_res(builder.create((hook_info, context.data.path.clone())))
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
                    .map(|acc| {
                        if acc {
                            HookExecution::Accepted
                        } else {
                            let desc = match t.get::<String, _, _>(2) {
                                Some(desc) => desc,
                                None => "".into(),
                            };
                            let long_desc = match t.get::<String, _, _>(3) {
                                Some(long_desc) => long_desc,
                                None => "".into(),
                            };
                            HookExecution::Rejected(HookRejectionInfo::new(desc, long_desc))
                        }
                    })
            })
            .flatten()
            .boxify()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use super::super::{HookChangeset, HookChangesetParents, InMemoryFileContentStore};
    use async_unit;
    use futures::Future;
    use mercurial_types::HgChangesetId;
    use std::str::FromStr;
    use std::sync::Arc;
    use test::to_mpath;

    #[test]
    fn test_cs_hook_simple_rejected() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return false\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(code, changeset),
                Ok(HookExecution::Rejected(_))
            );
        });
    }

    #[test]
    fn test_cs_hook_rejected_short_and_long_desc() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return false, \"emus\", \"ostriches\"\n\
                 end",
            );
            assert_matches!(
                run_changeset_hook(code, changeset),
                Ok(HookExecution::Rejected(HookRejectionInfo{ref description,
                    ref long_description}))
                    if description==&"emus" && long_description==&"ostriches"
            );
        });
    }

    #[test]
    fn test_cs_hook_author() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.info.author == \"some-author\"\n\
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
                "hook = function (ctx)\n\
                 return ctx.files[0] == nil and ctx.files[1] == \"file1\" and\n\
                 ctx.files[2] == \"file2\" and ctx.files[3] == \"file3\" and\n\
                 ctx.files[4] == nil\n\
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
                "hook = function (ctx)\n\
                 return ctx.info.comments == \"some-comments\"\n\
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
                "hook = function (ctx)\n\
                 return ctx.info.repo_name == \"some-repo\"\n\
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
                "hook = function (ctx)\n\
                 return ctx.info.parent1_hash == \"p1-hash\" and \n\
                 ctx.info.parent2_hash == nil\n\
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
                "hook = function (ctx)\n\
                 return ctx.info.parent1_hash == \"p1-hash\" and \n\
                 ctx.info.parent2_hash == \"p2-hash\"\n\
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
                "hook = function (ctx)\n\
                 return ctx.info.parent1_hash == nil and \n\
                 ctx.info.parent2_hash == nil\n\
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
                "elephants = function (ctx)\n\
                 return true\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_changeset_hook(code, changeset).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref msg)) if msg.contains("no hook function")
             );
        });
    }

    #[test]
    fn test_cs_hook_invalid_hook() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from("invalid code");
            assert_matches!(
                err_downcast!(run_changeset_hook(code, changeset).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookParseError(ref err_msg))
                    if err_msg.starts_with("Syntax error:")
             );
        });
    }

    #[test]
    fn test_cs_hook_exception() {
        async_unit::tokio_unit_test(|| {
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
                err_downcast!(run_changeset_hook(code, changeset).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg))
                    if err_msg.starts_with("LuaError")
             );
        });
    }

    #[test]
    fn test_cs_hook_invalid_return_val() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return \"aardvarks\"\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_changeset_hook(code, changeset).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg))
                    if err_msg.contains("invalid hook return type")
             );
        });
    }

    #[test]
    fn test_cs_hook_invalid_short_desc() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return false, 23, \"long desc\"\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_changeset_hook(code, changeset).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg))
                    if err_msg.contains("invalid hook failure short description type")
            );
        });
    }

    #[test]
    fn test_cs_hook_invalid_long_desc() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return false, \"short desc\", 23\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_changeset_hook(code, changeset).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg))
                    if err_msg.contains("invalid hook failure long description type")
            );
        });
    }

    #[test]
    fn test_cs_hook_desc_when_hooks_is_accepted() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return true, \"short\", \"long\"\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_changeset_hook(code, changeset).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg))
                    if err_msg.contains("failure description must only be set if hook fails")
             );
        });
    }

    #[test]
    fn test_cs_hook_long_desc_when_hooks_is_accepted() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (ctx)\n\
                 return true, nil, \"long\"\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_changeset_hook(code, changeset).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg))
                    if err_msg.contains("failure long description must only be set if hook fails")
             );
        });
    }

    #[test]
    fn test_file_hook_path() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file.path == \"/a/b/c.txt\"\n\
                 end",
            );
            assert_matches!(run_file_hook(code, hook_file), Ok(HookExecution::Accepted));
        });
    }

    #[test]
    fn test_file_hook_content_matches() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file.contains_string(\"sausages\")\n\
                 end",
            );
            assert_matches!(run_file_hook(code, hook_file), Ok(HookExecution::Accepted));
        });
    }

    #[test]
    fn test_file_hook_content_no_matches() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file.contains_string(\"gerbils\")\n\
                 end",
            );
            assert_matches!(
                run_file_hook(code, hook_file),
                Ok(HookExecution::Rejected(_))
            );
        });
    }

    #[test]
    fn test_file_hook_len_matches() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from(
                "hook = function (ctx)\n\
                print(\"length is \", ctx.file.len())\n
                 return ctx.file.len() == 8\n\
                 end",
            );
            assert_matches!(run_file_hook(code, hook_file), Ok(HookExecution::Accepted));
        });
    }

    #[test]
    fn test_file_hook_len_no_matches() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.file.len() == 123\n\
                 end",
            );
            assert_matches!(
                run_file_hook(code, hook_file),
                Ok(HookExecution::Rejected(_))
            );
        });
    }

    #[test]
    fn test_file_hook_repo_name() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return ctx.info.repo_name == \"some-repo\"\n\
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
                "hook = function (ctx)\n\
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
                "elephants = function (ctx)\n\
                 return true\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_file_hook(code, hook_file).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg)) if err_msg.contains("no hook function")
             );
        });
    }

    #[test]
    fn test_file_hook_invalid_hook() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from("invalid code");
            assert_matches!(
                err_downcast!(run_file_hook(code, hook_file).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookParseError(ref err_msg))
                    if err_msg.starts_with("Syntax error:")
             );
        });
    }

    #[test]
    fn test_file_hook_exception() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 if ctx.file.path == \"/a/b/c.txt\" then\n\
                 error(\"fubar\")\n\
                 end\n\
                 return true\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_file_hook(code, hook_file).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg))
                    if err_msg.starts_with("LuaError")
             );
        });
    }

    #[test]
    fn test_file_hook_invalid_return_val() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return \"aardvarks\"\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_file_hook(code, hook_file).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg))
                    if err_msg.contains("invalid hook return type")
             );
        });
    }

    #[test]
    fn test_file_hook_invalid_short_desc() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return false, 23, \"long desc\"\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_file_hook(code, hook_file).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg))
                    if err_msg.contains("invalid hook failure short description type")
            );
        });
    }

    #[test]
    fn test_file_hook_invalid_long_desc() {
        async_unit::tokio_unit_test(|| {
            let hook_file = default_hook_file();
            let code = String::from(
                "hook = function (ctx)\n\
                 return false, \"short desc\", 23\n\
                 end",
            );
            assert_matches!(
                err_downcast!(run_file_hook(code, hook_file).unwrap_err(), err: ErrorKind => err),
                Ok(ErrorKind::HookRuntimeError(ref err_msg))
                    if err_msg.contains("invalid hook failure long description type")
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
        let mut content_store = InMemoryFileContentStore::new();
        let cs_id = HgChangesetId::from_str("473b2e715e0df6b2316010908879a3c78e275dd9").unwrap();
        content_store.insert((cs_id.clone(), to_mpath("/a/b/c.txt")), "sausages".into());
        HookFile::new("/a/b/c.txt".into(), Arc::new(content_store), cs_id)
    }

}

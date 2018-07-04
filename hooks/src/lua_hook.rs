// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This sub module contains a Lua implementation of hooks

#![deny(warnings)]

use super::{Hook, HookChangesetParents, HookContext, HookExecution, HookRejectionInfo};
use failure::Error;
use ffi;
use futures::{failed, Future};
use futures_ext::{BoxFuture, FutureExt};
use hlua::{AsLua, Lua, LuaError, LuaRead, PushGuard};
use hlua_futures::{LuaCoroutine, LuaCoroutineBuilder};
use std::ffi::CString;

#[derive(Clone)]
pub struct LuaHook {
    pub name: String,
    /// The Lua code of the hook
    pub code: String,
}

impl Hook for LuaHook {
    fn run(&self, context: HookContext) -> BoxFuture<HookExecution, Error> {
        let hook = (*self).clone();
        let hook_name = hook.name.clone();
        match self.create_coroutine(hook, context) {
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
}

impl LuaHook {
    pub fn new(name: String, code: String) -> LuaHook {
        LuaHook { name, code }
    }

    fn create_coroutine<'lua>(
        &self,
        hook: LuaHook,
        context: HookContext,
    ) -> Result<LuaCoroutine<PushGuard<Lua<'lua>>, bool>, Error> {
        let mut lua = Lua::new();
        lua.openlibs();
        let res: Result<(), LuaError> = lua.execute::<()>(&hook.code);
        let res: Result<(), Error> = res.map_err(|e| {
            ErrorKind::HookParseError(hook.name.clone().into(), e.to_string()).into()
        });
        res?;
        let builder: LuaCoroutineBuilder<PushGuard<Lua<'lua>>> =
            match self.get_function(lua, "hook") {
                Some(val) => val,
                None => {
                    let err: Error =
                        ErrorKind::NoHookFunctionError(hook.name.clone().into()).into();
                    bail_err!(err)
                }
            };

        let mut hook_info = hashmap! {
            "repo_name" => context.repo_name.to_string(),
            "author" => context.changeset.author.to_string(),
            "comments" => context.changeset.comments.to_string(),
        };
        match context.changeset.parents {
            HookChangesetParents::None => (),
            HookChangesetParents::One(ref parent1_hash) => {
                hook_info.insert("parent1_hash", parent1_hash.to_string());
            }
            HookChangesetParents::Two(ref parent1_hash, ref parent2_hash) => {
                hook_info.insert("parent1_hash", parent1_hash.to_string());
                hook_info.insert("parent2_hash", parent2_hash.to_string());
            }
        }
        builder
            .create((hook_info, context.changeset.files.clone()))
            .map_err(|err| {
                ErrorKind::HookRuntimeError(hook.name.clone().into(), format!("{:?}", err)).into()
            })
    }

    // We can't use the Lua::get method to get the function as this method borrows Lua.
    // We need a method that moves the Lua instance into the builder and later the coroutine
    // future, as the future can't refer to structs with non static lifetimes
    // So this method use the Lua ffi directly to get the function which we pass to the
    // coroutine builder
    fn get_function<'lua, V>(&self, lua: Lua<'lua>, index: &str) -> Option<V>
    where
        V: LuaRead<PushGuard<Lua<'lua>>>,
    {
        let index = CString::new(index).unwrap();
        let guard = unsafe {
            ffi::lua_getglobal(lua.as_lua().state_ptr(), index.as_ptr());
            if ffi::lua_isnil(lua.as_lua().state_ptr(), -1) {
                let _guard = PushGuard::new(lua, 1);
                return None;
            }
            PushGuard::new(lua, 1)
        };
        // Calls lua_read on the coroutine builder
        // The builder later moves the Lua instance into the actual coroutine future when
        // create is called
        LuaRead::lua_read(guard).ok()
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
    use std::sync::Arc;

    fn default_changeset() -> HookChangeset {
        let files = vec!["file1".into(), "file2".into(), "file3".into()];
        HookChangeset::new(
            "some-author".into(),
            files,
            "some-comments".into(),
            HookChangesetParents::One("p1-hash".into()),
        )
    }

    #[test]
    fn test_rejected() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (info, files)\n\
                 return info.author == \"mr blobby\"\n\
                 end",
            );
            assert_matches!(run_hook(code, changeset), Ok(HookExecution::Rejected(_)));
        });
    }

    #[test]
    fn test_author() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (info, files)\n\
                 return info.author == \"some-author\"\n\
                 end",
            );
            assert_matches!(run_hook(code, changeset), Ok(HookExecution::Accepted));
        });
    }

    #[test]
    fn test_files() {
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
            assert_matches!(run_hook(code, changeset), Ok(HookExecution::Accepted));
        });
    }

    #[test]
    fn test_comments() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (info, files)\n\
                 return info.comments == \"some-comments\"\n\
                 end",
            );
            assert_matches!(run_hook(code, changeset), Ok(HookExecution::Accepted));
        });
    }

    #[test]
    fn test_repo_name() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (info, files)\n\
                 return info.repo_name == \"some-repo\"\n\
                 end",
            );
            assert_matches!(run_hook(code, changeset), Ok(HookExecution::Accepted));
        });
    }

    #[test]
    fn test_one_parent() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (info, files)\n\
                 return info.parent1_hash == \"p1-hash\" and \n\
                 info.parent2_hash == nil\n\
                 end",
            );
            assert_matches!(run_hook(code, changeset), Ok(HookExecution::Accepted));
        });
    }

    #[test]
    fn test_two_parents() {
        async_unit::tokio_unit_test(|| {
            let mut changeset = default_changeset();
            changeset.parents = HookChangesetParents::Two("p1-hash".into(), "p2-hash".into());
            let code = String::from(
                "hook = function (info, files)\n\
                 return info.parent1_hash == \"p1-hash\" and \n\
                 info.parent2_hash == \"p2-hash\"\n\
                 end",
            );
            assert_matches!(run_hook(code, changeset), Ok(HookExecution::Accepted));
        });
    }

    #[test]
    fn test_no_parents() {
        async_unit::tokio_unit_test(|| {
            let mut changeset = default_changeset();
            changeset.parents = HookChangesetParents::None;
            let code = String::from(
                "hook = function (info, files)\n\
                 return info.parent1_hash == nil and \n\
                 info.parent2_hash == nil\n\
                 end",
            );
            assert_matches!(run_hook(code, changeset), Ok(HookExecution::Accepted));
        });
    }

    #[test]
    fn test_no_hook_func() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "elephants = function (info, files)\n\
                 return true\n\
                 end",
            );
            assert_matches!(
                run_hook(code, changeset).unwrap_err().downcast::<ErrorKind>(),
                Ok(ErrorKind::NoHookFunctionError(ref hook_name)) if hook_name == "testhook"
             );
        });
    }

    #[test]
    fn test_invalid_hook() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from("invalid code");
            assert_matches!(
                run_hook(code, changeset).unwrap_err().downcast::<ErrorKind>(),
                Ok(ErrorKind::HookParseError(ref hook_name, ref err_msg))
                    if hook_name == "testhook" && err_msg.starts_with("Syntax error:")
             );
        });
    }

    #[test]
    fn test_hook_exception() {
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
                run_hook(code, changeset).unwrap_err().downcast::<ErrorKind>(),
                Ok(ErrorKind::HookRuntimeError(ref hook_name, ref err_msg))
                    if hook_name == "testhook" && err_msg.starts_with("LuaError")
             );
        });
    }

    #[test]
    fn test_invalid_return_val() {
        async_unit::tokio_unit_test(|| {
            let changeset = default_changeset();
            let code = String::from(
                "hook = function (info, files)\n\
                 return \"aardvarks\"\n\
                 end",
            );
            assert_matches!(
                run_hook(code, changeset).unwrap_err().downcast::<ErrorKind>(),
                Ok(ErrorKind::HookRuntimeError(ref hook_name, ref err_msg))
                    if hook_name == "testhook" && err_msg.starts_with("LuaError")
             );
        });
    }

    fn run_hook(code: String, changeset: HookChangeset) -> Result<HookExecution, Error> {
        let hook = LuaHook::new(String::from("testhook"), code.to_string());
        let context = HookContext::new(hook.name.clone(), "some-repo".into(), Arc::new(changeset));
        hook.run(context).wait()
    }
}

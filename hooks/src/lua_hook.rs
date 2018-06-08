// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This sub module contains a Lua implementation of hooks

#![deny(warnings)]

use super::{Hook, HookChangesetParents, HookContext, HookExecution, HookRejectionInfo};
use failure::Error;
use futures_ext::{asynchronize, BoxFuture};
use hlua::{Lua, LuaError, LuaFunction};

#[derive(Clone)]
pub struct LuaHook {
    pub name: String,
    /// The Lua code of the hook
    pub code: String,
}

impl Hook for LuaHook {
    fn run(&self, context: HookContext) -> BoxFuture<HookExecution, Error> {
        let hook = (*self).clone();
        // The Lua hook function may block waiting for a coroutine to yield
        // (e.g. if the hook makes a network call) so we need to run it on a thread from
        // the thread pool. LuaCoroutines can't be passed to different threads
        // TODO thread pool should be configurable, not always the default
        let fut: BoxFuture<HookExecution, Error> =
            asynchronize(move || LuaHook::run_hook(hook, context));
        fut
    }
}

impl LuaHook {
    fn run_hook(hook: LuaHook, context: HookContext) -> Result<HookExecution, Error> {
        println!("Running lua hook {}", hook.name);
        let mut lua = Lua::new();
        lua.openlibs();
        let res: Result<(), LuaError> = lua.execute::<()>(&hook.code);
        let res: Result<(), Error> = res.map_err(|e| {
            ErrorKind::HookParseError(hook.name.clone().into(), e.to_string()).into()
        });
        res?;
        let mut hook_func: LuaFunction<_> = match lua.get("hook") {
            Some(val) => val,
            None => bail_err!(ErrorKind::NoHookFunctionError(hook.name.clone().into())),
        };
        let mut hook_info = hashmap! {
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
        hook_func
            .call_with_args((hook_info, context.changeset.files.clone()))
            .map_err(|e| {
                ErrorKind::HookRuntimeError(hook.name.clone().into(), format!("{:?}", e).into())
                    .into()
            })
            .map(|b| {
                if b {
                    HookExecution::Accepted
                } else {
                    // TODO allow proper hook rejection to be set from Lua hook
                    HookExecution::Rejected(HookRejectionInfo::new(
                        "short desc".into(),
                        "long desc".into(),
                    ))
                }
            })
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
                "hook = function (info, files)
                    error(\"fubar\")
                    return true
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
                "hook = function (info, files)
                        return \"aardvarks\"
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
        let hook = LuaHook {
            name: String::from("testhook"),
            code: code.to_string(),
        };
        let context = HookContext::new(hook.name.clone(), Arc::new(changeset));
        hook.run(context).wait()
    }
}

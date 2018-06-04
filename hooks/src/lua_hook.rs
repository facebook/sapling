// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This sub module contains a Lua implementation of hooks

#![deny(warnings)]

use super::Hook;
use super::HookContext;

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
    fn run(&self, context: HookContext) -> BoxFuture<bool, Error> {
        let hook = (*self).clone();
        // The Lua hook function may block waiting for a coroutine to yield
        // (e.g. if the hook makes a network call) so we need to run it on a thread from
        // the thread pool. LuaCoroutines can't be passed to different threads
        // TODO thread pool should be configurable, not always the default
        let fut: BoxFuture<bool, Error> = asynchronize(move || LuaHook::run_hook(hook, context));
        fut
    }
}

impl LuaHook {
    fn run_hook(hook: LuaHook, context: HookContext) -> Result<bool, Error> {
        println!("Running lua hook {}", hook.name);
        let mut lua = Lua::new();
        lua.openlibs();
        let res: Result<(), LuaError> = lua.execute::<()>(&hook.code);
        let res: Result<(), Error> =
            res.map_err(|_| ErrorKind::HookDefinitionError("failed to parse hook".into()).into());
        res?;
        let mut hook_func: LuaFunction<_> = match lua.get("hook") {
            Some(val) => val,
            None => bail_err!(ErrorKind::HookDefinitionError(
                "global variable 'hook' not found".into(),
            )),
        };
        let hook_info = hashmap! {
            "author" => context.changeset.author.to_string(),
        };
        hook_func
            .call_with_args((hook_info, context.changeset.files.clone()))
            .map_err(|err| {
                ErrorKind::HookRuntimeError(hook.name.clone().into(), format!("{:?}", err)).into()
            })
    }
}

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Hook definition error: {}", _0)] HookDefinitionError(String),
    #[fail(display = "Error while running hook '{}': {}", _0, _1)] HookRuntimeError(String, String),
}

#[cfg(test)]
mod test {
    use super::*;
    use super::super::HookChangeset;
    use async_unit;
    use futures::Future;
    use std::sync::Arc;

    #[test]
    fn test_author() {
        async_unit::tokio_unit_test(|| {
            let files = vec![String::from("filec")];
            let author = String::from("jane bloggs");
            let code = String::from(
                "hook = function (info, files)
    return info.author == \"jane bloggs\"
end",
            );
            run_hook_test(code, author, files, true);
        });
    }

    #[test]
    fn test_files() {
        async_unit::tokio_unit_test(|| {
            let files = vec![
                String::from("filec"),
                String::from("fileb"),
                String::from("filed"),
                String::from("filez"),
            ];
            let author = String::from("whatevs");
            // Arrays passed from rust -> lua appear to be 1 indexed in Lua land
            let code = String::from(
                "hook = function (info, files)
    return files[0] == nil and files[1] == \"filec\" and
                         files[2] == \"fileb\" and files[3] == \"filed\" and
                         files[4] == \"filez\" and files[5] == nil
end",
            );
            println!("code {}", code);
            run_hook_test(code, author, files, true);
        });
    }

    fn run_hook_test(code: String, author: String, files: Vec<String>, expected: bool) {
        let hook = LuaHook {
            name: String::from("testhook"),
            code: code.to_string(),
        };
        let changeset = HookChangeset::new(author, files);
        let context = HookContext::new(hook.name.clone(), Arc::new(changeset));
        let res = hook.run(context).wait();
        match res {
            Ok(r) => assert!(r == expected),
            Err(e) => {
                println!("Failed to run hook {}", e);
                assert!(false); // Just fail
            }
        }
    }
}

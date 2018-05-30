// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This sub module contains a Lua implementation of hooks

#![deny(warnings)]

use super::Hook;
use super::HookChangeset;
use super::HookRunner;

use failure::{Error, Result};
use futures_ext::BoxFuture;
use futures_ext::asynchronize;
use hlua::Lua;
use std::sync::Arc;

pub struct LuaHookRunner {}

pub struct LuaHook {
    pub name: String,
    /// The Lua code of the hook
    pub code: String,
}

/// Implementation of HookRunner which knows how to run hooks written in Lua
impl HookRunner for LuaHookRunner {
    fn run_hook(
        self: &Self,
        hook: Box<Hook>,
        changeset: Arc<HookChangeset>,
    ) -> BoxFuture<bool, Error> {
        let fut = asynchronize(move || hook.run(changeset));
        Box::new(fut)
    }
}

impl LuaHookRunner {
    pub fn new() -> Self {
        LuaHookRunner {}
    }

    pub fn new_hook(&self, name: String, code: String) -> LuaHook {
        LuaHook { name, code }
    }
}

impl Hook for LuaHook {
    fn run(&self, changeset: Arc<HookChangeset>) -> Result<bool> {
        println!("Running lua hook {}", self.name);
        let mut lua = Lua::new();
        lua.openlibs();
        lua.set("user", changeset.user.clone());
        lua.set("files", changeset.files.clone());
        Ok(lua.execute(&self.code)?)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use super::super::*;
    use futures::Future;

    #[test]
    fn test_user() {
        async_unit::tokio_unit_test(|| {
            let files = vec![String::from("filec")];
            let user = String::from("jane bloggs");
            let code = String::from("return user == \"jane bloggs\"");
            run_hook_test(code, user, files, true);
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
            let user = String::from("whatevs");
            // Arrays passed from rust -> lua appear to be 1 indexed in Lua land
            let code = String::from(
                "return files[0] == nil and files[1] == \"filec\" and
                                     files[2] == \"fileb\" and files[3] == \"filed\" and
                                     files[4] == \"filez\" and files[5] == nil",
            );
            println!("code {}", code);
            run_hook_test(code, user, files, true);
        });
    }

    fn run_hook_test(code: String, user: String, files: Vec<String>, expected: bool) {
        let hook_runner = LuaHookRunner::new();
        let hook = hook_runner.new_hook(String::from("testhook"), code.to_string());
        let changeset = HookChangeset::new(user, files);
        let fut = hook_runner.run_hook(Box::new(hook), Arc::new(changeset));
        let res = fut.wait();
        match res {
            Ok(r) => assert!(r == expected),
            Err(e) => {
                println!("Failed to run hook {}", e);
                assert!(false); // Just fail
            }
        }
    }
}

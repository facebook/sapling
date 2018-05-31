// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This sub module contains a Lua implementation of hooks

#![deny(warnings)]

use super::Hook;
use super::HookChangeset;

use failure::Result;
use hlua::Lua;
use std::sync::Arc;

pub struct LuaHook {
    pub name: String,
    /// The Lua code of the hook
    pub code: String,
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

    #[test]
    fn test_user() {
        let files = vec![String::from("filec")];
        let user = String::from("jane bloggs");
        let code = String::from("return user == \"jane bloggs\"");
        run_hook_test(code, user, files, true);
    }

    #[test]
    fn test_files() {
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
    }

    fn run_hook_test(code: String, user: String, files: Vec<String>, expected: bool) {
        let hook = LuaHook {
            name: String::from("testhook"),
            code: code.to_string(),
        };
        let changeset = HookChangeset::new(user, files);
        let res = hook.run(Arc::new(changeset));
        match res {
            Ok(r) => assert!(r == expected),
            Err(e) => {
                println!("Failed to run hook {}", e);
                assert!(false); // Just fail
            }
        }
    }
}

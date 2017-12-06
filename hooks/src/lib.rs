// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Support for running hooks.
#![deny(warnings)]

extern crate ascii;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate hlua;
#[cfg_attr(test, macro_use)]
extern crate maplit;
#[cfg(test)]
extern crate tempdir;

extern crate hlua_futures;
extern crate mercurial;
extern crate mercurial_types;

mod errors;

use std::collections::HashMap;
use std::sync::Arc;

use ascii::IntoAsciiString;
use failure::ResultExt;
use futures::Future;
use hlua::{AnyLuaValue, Lua, LuaError, PushGuard};

use hlua_futures::{AnyFuture, LuaCoroutine, LuaCoroutineBuilder};
use mercurial_types::{Changeset, NodeHash, Repo};

pub use errors::*;

#[allow(dead_code)]
pub struct HookInfo {
    pub repo: String,
    pub bookmark: String,
    pub old_hash: NodeHash,
    pub new_hash: NodeHash,
}

pub struct HookManager<'lua> {
    // TODO: multiple contexts
    lua: Lua<'lua>,
}

pub struct HookContext<'hook, R: Repo> {
    name: &'hook str,
    repo: Arc<R>,
    info: HashMap<&'static str, String>,
    code: &'hook str,
}

impl<'hook, R: Repo> HookContext<'hook, R> {
    fn run<'a, 'lua>(
        &self,
        lua: &'a mut Lua<'lua>,
    ) -> Result<LuaCoroutine<PushGuard<&'a mut Lua<'lua>>, bool>> {
        let repo = self.repo.clone();
        let name = self.name.to_string();

        let get_author = move |hash: String| -> Result<AnyFuture> {
            let hash = hash.into_ascii_string().map_err(|hash| {
                ErrorKind::InvalidHash(name.clone(), hash.into_source())
            })?;
            let hash = NodeHash::from_ascii_str(&hash)
                .with_context(|_| ErrorKind::InvalidHash(name.clone(), hash.into()))?;

            let future = repo.get_changeset_by_nodeid(&hash)
                .map_err(|err| {
                    LuaError::ExecutionError(format!("failed to get author: {}", err))
                })
                .map(|cs| {
                    AnyLuaValue::LuaString(String::from_utf8_lossy(cs.user()).into_owned())
                });
            Ok(AnyFuture::new(future))
        };
        lua.set("get_author", hlua::function1(get_author));

        lua.execute::<()>(self.code)?;

        let builder: LuaCoroutineBuilder<_> = match lua.get("hook") {
            Some(val) => val,
            None => Err(ErrorKind::HookDefinitionError(
                "function 'hook' not found".into(),
            ))?,
        };
        // TODO: do we really need the clone?
        // TODO: use chain_err once LuaFunctionCallError implements std::error::Error
        let coroutine_fut = builder.create(self.info.clone()).map_err(|err| {
            ErrorKind::HookRuntimeError(self.name.into(), format!("{:?}", err)).into()
        });
        coroutine_fut
    }
}

impl<'lua> HookManager<'lua> {
    pub fn new() -> Self {
        let mut lua = Lua::new();
        // TODO: don't open all libs
        lua.openlibs();

        HookManager { lua }
    }

    pub fn run_hook<'hook, R: Repo>(
        &mut self,
        hook: HookContext<'hook, R>,
    ) -> Result<LuaCoroutine<PushGuard<&mut Lua<'lua>>, bool>> {
        // TODO: with multiple Lua contexts, choose a context to run in. Probably use a queue or
        // something.
        hook.run(&mut self.lua)
    }
}

#[cfg(test)]
mod test {
    use std::fs::File;
    use std::path::Path;
    use std::process::Command;

    use tempdir::TempDir;

    use super::*;

    #[test]
    fn test_hook() {
        let (hash, dir) = create_repo();
        let dot_hg = dir.as_ref().join(".hg");

        let hook_info = hashmap! {
            "repo" => "fbsource".into(),
            "bookmark" => "master".into(),
            "old_hash" => "0000000000000000000000000000000000000000".into(),
            "new_hash" => hash,
        };
        let mut hook_manager = HookManager::new();
        let repo = match mercurial::RevlogRepo::open(&dot_hg) {
            Ok(repo) => repo,
            Err(err) => panic!("RevlogRepo::open({}) failed {:?}", dot_hg.display(), err),
        };
        let hook = HookContext {
            name: "test",
            repo: Arc::new(repo),
            info: hook_info,
            code: "
                    function hook(info)
                        if info.repo ~= \"fbsource\" then
                            return false
                        else
                            author = coroutine.yield(get_author(info.new_hash))
                            return author == \"testuser\"
                        end
                    end",
        };

        let coroutine_fut = hook_manager.run_hook(hook).unwrap();
        let result = coroutine_fut.wait();
        assert!(result.unwrap());
    }

    fn create_repo() -> (String, TempDir) {
        // XXX replace this with a valid prebuilt repo
        let dir = TempDir::new("mononoke-hooks").unwrap();
        let status = Command::new("hg")
            .arg("init")
            .current_dir(&dir)
            .status()
            .expect("hg init failed");
        assert!(status.success());

        {
            let new_file = dir.as_ref().join("foo.txt");
            File::create(new_file).unwrap();
        }
        let status = hg_cmd(&dir)
            .arg("add")
            .arg("foo.txt")
            .status()
            .expect("hg add failed");
        assert!(status.success());

        let status = hg_cmd(&dir)
            .arg("commit")
            .arg("-utestuser")
            .arg("-mtest")
            .status()
            .expect("hg commit failed");
        assert!(status.success());

        // Get the new hash and return it.
        let output = hg_cmd(&dir)
            .arg("log")
            .arg("-r.")
            .arg("-T{node}")
            .output()
            .expect("hg log failed");
        assert!(output.status.success());
        let stdout = output.stdout;

        (String::from_utf8(stdout).unwrap(), dir)
    }

    fn hg_cmd<P: AsRef<Path>>(dir: P) -> Command {
        let mut command = Command::new("hg");
        command.env("HGPLAIN", "1");
        command.current_dir(dir);
        command
    }
}

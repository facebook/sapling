// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Support for running hooks.
#![deny(warnings)]

extern crate ascii;
#[cfg(test)]
extern crate async_unit;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate hlua;
#[cfg_attr(test, macro_use)]
extern crate maplit;
#[cfg(test)]
extern crate tempdir;

extern crate blobrepo;
extern crate hlua_futures;
extern crate mercurial;
extern crate mercurial_types;

#[cfg(test)]
extern crate fixtures;

mod errors;

use std::collections::HashMap;
use std::sync::Arc;

use ascii::IntoAsciiString;
use failure::ResultExt;
use futures::Future;
use hlua::{AnyLuaValue, Lua, LuaError, PushGuard};

use blobrepo::BlobRepo;
use hlua_futures::{AnyFuture, LuaCoroutine, LuaCoroutineBuilder};
use mercurial_types::{Changeset, HgNodeHash};
use mercurial_types::nodehash::HgChangesetId;

pub use errors::*;

#[allow(dead_code)]
pub struct HookInfo {
    pub repo: String,
    pub bookmark: String,
    pub old_hash: HgNodeHash,
    pub new_hash: HgNodeHash,
}

pub struct HookManager<'lua> {
    // TODO: multiple contexts
    lua: Lua<'lua>,
}

pub struct HookContext<'hook> {
    name: &'hook str,
    repo: Arc<BlobRepo>,
    info: HashMap<&'static str, String>,
    code: &'hook str,
}

impl<'hook> HookContext<'hook> {
    fn run<'a, 'lua>(
        &self,
        lua: &'a mut Lua<'lua>,
    ) -> Result<LuaCoroutine<PushGuard<&'a mut Lua<'lua>>, bool>> {
        let repo = self.repo.clone();
        let name = self.name.to_string();

        let get_author = move |hash: String| -> Result<AnyFuture> {
            let hash = hash.into_ascii_string()
                .map_err(|hash| ErrorKind::InvalidHash(name.clone(), hash.into_source()))?;
            let changesetid = HgChangesetId::from_ascii_str(&hash)
                .with_context(|_| ErrorKind::InvalidHash(name.clone(), hash.into()))?;

            let future = repo.get_changeset_by_changesetid(&changesetid)
                .map_err(|err| LuaError::ExecutionError(format!("failed to get author: {}", err)))
                .map(|cs| AnyLuaValue::LuaString(String::from_utf8_lossy(cs.user()).into_owned()));
            Ok(AnyFuture::new(future))
        };
        lua.set("get_author", hlua::function1(get_author));

        lua.execute::<()>(self.code)?;

        let builder: LuaCoroutineBuilder<_> = match lua.get("hook") {
            Some(val) => val,
            None => bail_err!(ErrorKind::HookDefinitionError(
                "function 'hook' not found".into(),
            )),
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

    pub fn run_hook<'hook>(
        &mut self,
        hook: HookContext<'hook>,
    ) -> Result<LuaCoroutine<PushGuard<&mut Lua<'lua>>, bool>> {
        // TODO: with multiple Lua contexts, choose a context to run in. Probably use a queue or
        // something.
        hook.run(&mut self.lua)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_hook() {
        async_unit::tokio_unit_test(|| {
            let hook_info = hashmap! {
                "repo" => "fbsource".into(),
                "bookmark" => "master".into(),
                "old_hash" => "0000000000000000000000000000000000000000".into(),
                "new_hash" => "a5ffa77602a066db7d5cfb9fb5823a0895717c5a".into(),
            };
            let mut hook_manager = HookManager::new();
            let repo = fixtures::linear::getrepo(None);
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
                            return author == \"Jeremy Fitzhardinge <jsgf@fb.com>\"
                        end
                    end",
            };

            let coroutine_fut = hook_manager.run_hook(hook).unwrap();
            let result = coroutine_fut.wait();
            assert!(result.unwrap());
        })
    }
}

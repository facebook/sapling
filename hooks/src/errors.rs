// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use hlua;

#[recursion_limit = "1024"]
error_chain! {
    errors {
        LuaError {
            description("Lua error")
            display("Lua error")
        }
        HookDefinitionError(err: String) {
            description("Hook definition error")
            display("Hook definition error: {}", err)
        }
        HookRuntimeError(hook_name: String, err: String) {
            description("Error while running hook")
            display("Error while running hook '{}': {}", hook_name, err)
        }
        InvalidHash(hook_name: String, hash: String) {
            description("Error while running hook: invalid hash")
            display("Error while running hook '{}': invalid hash '{}'", hook_name, hash)
        }
    }

    foreign_links {
        Lua(hlua::LuaError);
    }
}

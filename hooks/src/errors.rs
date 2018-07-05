// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "No changeset with id '{}'", _0)] NoSuchChangeset(String),
    #[fail(display = "No such hook '{}'", _0)] NoSuchHook(String),

    #[fail(display = "Error while parsing hook '{}'", _0)] HookParseError(String),
    #[fail(display = "Error while running hook '{}'", _0)] HookRuntimeError(String),
}

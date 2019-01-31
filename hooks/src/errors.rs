// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bookmarks::Bookmark;
pub use mercurial_types::HgChangesetId;
pub use mononoke_types::MPath;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "No changeset with id '{}'", _0)]
    NoSuchChangeset(String),
    #[fail(display = "No such hook '{}'", _0)]
    NoSuchHook(String),

    #[fail(display = "Error while parsing hook '{}'", _0)]
    HookParseError(String),
    #[fail(display = "Error while running hook '{}'", _0)]
    HookRuntimeError(String),

    #[fail(display = "invalid file structure: {}", _0)]
    InvalidFileStructure(String),
    #[fail(display = "invalid path: {}", _0)]
    InvalidPath(MPath),

    #[fail(display = "No file content for '{}'", _0)]
    NoFileContent(HgChangesetId, MPath),

    #[fail(display = "Hook(s) referenced in bookmark {} do not exist", _0)]
    NoSuchBookmarkHook(Bookmark),

    #[fail(display = "invalid rust hook: {}", _0)]
    InvalidRustHook(String),
}

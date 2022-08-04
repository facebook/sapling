/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

pub use mercurial_types::HgChangesetId;
use metaconfig_types::BookmarkOrRegex;
pub use mononoke_types::MPath;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("No such hook '{0}'")]
    NoSuchHook(String),

    #[error("Error while parsing hook '{0}'")]
    HookParseError(String),
    #[error("Error while running hook '{0}'")]
    HookRuntimeError(String),

    #[error("invalid file structure: {0}")]
    InvalidFileStructure(String),
    #[error("invalid path: {0}")]
    InvalidPath(MPath),

    #[error("Missing file for cs '{0}' path '{1}'")]
    MissingFile(HgChangesetId, MPath),

    #[error("Hook(s) referenced in bookmark {0:#?} do not exist: {1:?}")]
    NoSuchBookmarkHook(BookmarkOrRegex, HashSet<String>),

    #[error("invalid rust hook: {0}")]
    InvalidRustHook(String),

    #[error("Disabled hook(s) do(es) not exist: {0:?}")]
    NoSuchHookToDisable(HashSet<String>),
}

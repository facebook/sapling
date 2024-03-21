/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod dotgit;
mod filesystem;

pub use dotgit::DotGitFileSystem;
pub use filesystem::FileSystem;
pub use filesystem::PendingChange;

#[derive(PartialEq)]
pub enum FileSystemType {
    Normal,
    Watchman,
    Eden,
}

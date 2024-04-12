/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod dotgit;
mod filesystem;
pub mod physicalfs;
pub mod watchmanfs;

pub use dotgit::DotGitFileSystem;
pub use filesystem::FileSystem;
pub use filesystem::PendingChange;
pub use physicalfs::PhysicalFileSystem;
pub use watchmanfs::WatchmanFileSystem;

#[cfg(feature = "eden")]
pub mod edenfs;
#[cfg(feature = "eden")]
pub use edenfs::EdenFileSystem;

#[derive(PartialEq)]
pub enum FileSystemType {
    Normal,
    Watchman,
    Eden,
    DotGit,
}

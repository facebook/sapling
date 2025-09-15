/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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

#[derive(Clone, PartialEq)]
pub enum FileSystemType {
    Normal,
    Watchman,
    Eden,
    DotGit,
}

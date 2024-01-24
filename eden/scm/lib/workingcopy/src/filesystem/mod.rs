/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod filesystem;

pub use filesystem::FileSystem;
pub use filesystem::PendingChange;

#[derive(PartialEq)]
pub enum FileSystemType {
    Normal,
    Watchman,
    Eden,
}

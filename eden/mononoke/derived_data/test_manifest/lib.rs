/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod derive;
mod mapping;
#[cfg(test)]
mod tests;

pub use mapping::format_key;
pub use mapping::RootTestManifestDirectory;

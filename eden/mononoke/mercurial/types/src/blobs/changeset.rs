/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod revlog;
pub use revlog::serialize_cs;
pub use revlog::serialize_extras;
pub use revlog::Extra;
pub use revlog::RevlogChangeset;

mod blob;
pub use blob::ChangesetMetadata;
pub use blob::HgBlobChangeset;
pub use blob::HgChangesetContent;

#[cfg(test)]
mod test;

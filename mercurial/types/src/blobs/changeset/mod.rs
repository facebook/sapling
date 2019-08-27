// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

mod revlog;
pub use revlog::{serialize_cs, serialize_extras, Extra, RevlogChangeset};

mod blob;
pub use blob::{ChangesetMetadata, HgBlobChangeset, HgChangesetContent};

#[cfg(test)]
mod test;

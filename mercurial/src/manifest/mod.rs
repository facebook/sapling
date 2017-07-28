// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

pub mod revlog;

pub use mercurial_types::{Manifest, Repo};
pub use self::revlog::{Details, RevlogManifest};

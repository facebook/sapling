// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::{Future, Stream};

/// A general source control Node
///
/// A `Node` has some content, and some number of `Parents` (immediate ancestors).
/// For Mercurial this is constrained to [0, 2] parents, but other scms (ie Git) can have
/// arbitrary numbers of parents.
pub trait Node: Sized {
    type Content;
    type Error;

    type GetParents: Stream<Item = Self, Error = Self::Error>;
    type GetContent: Future<Item = Self::Content, Error = Self::Error>;

    fn get_parents(&self) -> Self::GetParents;
    fn get_content(&self) -> Self::GetContent;
}

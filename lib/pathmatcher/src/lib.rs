// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate ignore;

#[cfg(test)]
extern crate tempdir;

mod gitignore_matcher;

pub use gitignore_matcher::GitignoreMatcher;

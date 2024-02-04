/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::path::MPath;

/// An iterator over all the prefixes of a path.
pub struct MononokePathPrefixes {
    next_path: Option<MPath>,
}

impl MononokePathPrefixes {
    pub fn new(path: &MPath) -> Self {
        let next_path = path.split_dirname().map(|(path, _)| path);
        MononokePathPrefixes { next_path }
    }
}

impl Iterator for MononokePathPrefixes {
    type Item = MPath;

    fn next(&mut self) -> Option<MPath> {
        match self.next_path.take() {
            None => None,
            Some(path) => {
                self.next_path = path.split_dirname().map(|(path, _)| path);
                Some(path)
            }
        }
    }
}

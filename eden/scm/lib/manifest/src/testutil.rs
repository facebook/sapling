/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use types::testutil::*;

use crate::File;
use crate::FileMetadata;

pub fn make_meta(hex: &str) -> FileMetadata {
    FileMetadata::regular(hgid(hex))
}

pub fn make_file(path: &str, hex: &str) -> File {
    File {
        path: repo_path_buf(path),
        meta: make_meta(hex),
    }
}

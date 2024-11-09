/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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

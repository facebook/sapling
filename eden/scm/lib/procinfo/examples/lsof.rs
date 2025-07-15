/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;

fn main() {
    #[cfg(target_os = "macos")]
    for file in std::env::args().skip(1) {
        println!(
            "{file}: {:?}",
            procinfo::macos::file_path_to_pid(Path::new(&file))
        );
    }
}

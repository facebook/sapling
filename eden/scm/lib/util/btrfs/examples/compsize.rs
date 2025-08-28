/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

fn main() {
    #[cfg(target_os = "linux")]
    for file in std::env::args().skip(1) {
        println!(
            "{file}: {:?}",
            btrfs::physical_size(&std::fs::File::open(&file).unwrap(), None)
        );
    }
}

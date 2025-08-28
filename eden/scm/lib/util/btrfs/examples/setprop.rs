/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

fn main() {
    #[cfg(target_os = "linux")]
    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let file = std::fs::File::open(&args[0]).unwrap();
        println!("{:?}", btrfs::set_property(&file, &args[1], &args[2]));
    }
}

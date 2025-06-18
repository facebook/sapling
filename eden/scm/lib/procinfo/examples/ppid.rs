/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

fn main() {
    let mut ppid = procinfo::parent_pid(0);
    while ppid != 0 {
        let name = procinfo::exe_name(ppid);
        println!("Parent PID: {:8}  Name: {}", ppid, name);
        ppid = procinfo::parent_pid(ppid);
    }

    println!("Compact:\n{}", procinfo::ancestors(0));
}

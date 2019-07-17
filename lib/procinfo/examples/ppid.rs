// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

fn main() {
    let mut ppid = procinfo::parent_pid(0);
    while ppid != 0 {
        println!("Parent PID: {}", ppid);
        ppid = procinfo::parent_pid(ppid);
    }
}

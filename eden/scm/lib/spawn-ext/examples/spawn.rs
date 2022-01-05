/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::process::Command;
use std::time::SystemTime;

use spawn_ext::CommandExt;

fn main() {
    let exe_path = std::env::current_exe().unwrap();
    if std::env::args().len() == 1 {
        let pid = unsafe { libc::getpid() };
        println!("parent pid: {}", pid);
        let mut cmd = Command::new(exe_path);
        let clock = SystemTime::now();
        cmd.arg("child").avoid_inherit_handles().new_session();
        println!("avoid_inherit_handles took: {:?}", clock.elapsed().unwrap());
        let child_pid = cmd.arg("child").spawn_detached().unwrap().id();
        println!("child pid: {}", child_pid);
        println!("spawn took: {:?}", clock.elapsed().unwrap());
        println!("Both processes are sleeping.");
        println!();
        if cfg!(windows) {
            println!("On Windows, use Process Hacker to check handles.");
            println!("Inheritable handles are highlighted in cyan.");
        } else {
            println!(
                "On Linux, use 'ls -l /proc/{{{},{}}}/fd/' to check fds.",
                pid, child_pid
            );
        }
        println!();
        println!("The child should not have more file handles than the parent.");
    }
    std::thread::sleep(std::time::Duration::from_secs(300))
}

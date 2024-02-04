/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;
use std::io::IsTerminal;
use std::io::Read;
use std::io::Write;
use std::mem;
use std::process::Command;
use std::process::Stdio;

use filedescriptor::AsRawFileDescriptor;
use filedescriptor::FileDescriptor;
use filedescriptor::FromRawFileDescriptor;
use filedescriptor::RawFileDescriptor;
use nodeipc::NodeIpc;
use serde_json::json;
use serde_json::Value;

fn main() {
    let is_child = env::args().nth(1).as_deref() == Some("child");

    if is_child {
        child_main();
    } else {
        parent_main();
    }
}

fn child_main() {
    let handle = env::args().nth(2).unwrap().parse::<u64>().unwrap() as RawFileDescriptor;
    println!("Child: started with IPC handle {:x}", handle as u64);

    let ipc = NodeIpc::from_raw_file_descriptor(handle).unwrap();
    // Needed to read from the inherited socketpair handle.
    maybe_init_winsock();

    let message: Option<String> = ipc.recv().unwrap();
    println!("Child: got message {:?}", message);

    let payload = ipc.recv_fd_vec().unwrap();
    println!("Child: got sendfd payload {:?}", &payload);

    for fd in payload.raw_fds {
        #[cfg(windows)]
        if fd.is_null() {
            continue;
        }
        println!("Child: writing \"something\\n\" to fd {:?}", fd);
        let mut fd = unsafe { FileDescriptor::from_raw_file_descriptor(fd) };
        if let Err(e) = fd.write_all(b"something\n") {
            println!("Child: write failed: {:?}", e);
        }
        // Do not make FileDescriptor close the handle. We might still need it for `println!`.
        mem::forget(fd);
    }

    ipc.recv_stdio().unwrap();
    println!("Child: got stdio");
    eprintln!("Child: write to stderr");

    if let Some(ipc) = nodeipc::get_singleton() {
        println!("Child: has IPC singleton");
        ipc.send("HELLO FROM CHILD").unwrap();
        while let Some(message) = ipc.recv::<Value>().unwrap() {
            println!("Child: Got message: {:?}", message);
            if message.as_str() == Some("BYE") {
                break;
            } else {
                ipc.send(json!(["Echo from child", message])).unwrap();
            }
        }
    } else {
        println!("Child: no IPC singleton");
    }

    let mut stdin = std::io::stdin();
    let stdin_is_terminal = stdin.is_terminal();
    println!("Child: stdin.is_terminal: {}", stdin_is_terminal);
    if !stdin_is_terminal {
        println!("Child: reading from stdin");
        let mut content = String::new();
        stdin.read_to_string(&mut content).unwrap();
        println!("Child: got: {:?}", content);
    }
}

fn parent_main() {
    let (server_socket, client_socket) = filedescriptor::socketpair().unwrap();
    let client_raw_fd = client_socket.as_raw_file_descriptor();

    // Make the client_socket inheritable.
    #[cfg(windows)]
    unsafe {
        use winapi::um::handleapi::SetHandleInformation;
        use winapi::um::winbase::HANDLE_FLAG_INHERIT;
        SetHandleInformation(client_raw_fd as _, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
    }
    #[cfg(unix)]
    unsafe {
        let flags = libc::fcntl(client_raw_fd, libc::F_GETFD);
        let new_flags = flags & !libc::FD_CLOEXEC;
        let ret = libc::fcntl(client_raw_fd, libc::F_SETFD, new_flags);
        assert!(ret == 0);
    }

    let ipc = NodeIpc::from_socket(server_socket).unwrap();

    println!("Parent: spawning child");
    let mut child = Command::new(env::current_exe().unwrap())
        .arg("child")
        .arg((client_raw_fd as u64).to_string())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    drop(client_socket);

    println!("Parent: sending hello");
    ipc.send("hello").unwrap();

    println!("Parent: sending a.txt file descriptor");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("a.txt")
        .unwrap();
    let fds = [file.as_raw_file_descriptor()];
    ipc.send_fd_vec(&fds).unwrap();

    println!("Parent: sending stdio");
    ipc.send_stdio().unwrap();

    println!("Parent: waiting for child to exit");
    child.wait().unwrap();
}

fn maybe_init_winsock() {
    #[cfg(windows)]
    unsafe {
        use winapi::um::winsock2::WSAStartup;
        use winapi::um::winsock2::WSADATA;

        let mut data: WSADATA = mem::zeroed();
        let ret = WSAStartup(
            0x202, // version 2.2
            &mut data,
        );
        assert_eq!(ret, 0, "failed to initialize winsock");
    }
}

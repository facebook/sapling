/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use nodeipc::IPC;
use serde_json::json;
use serde_json::Value;

fn main() {
    if let Some(ipc) = &*IPC {
        ipc.send("HELLO FROM CHILD").unwrap();
        while let Some(message) = ipc.recv::<Value>().unwrap() {
            println!("[Child] Got message from parent: {:?}", message);
            if message.as_str() == Some("BYE") {
                break;
            } else {
                ipc.send(json!(["Echo from child", message])).unwrap();
            }
        }
    } else {
        println!("[Child] IPC is not setup. Is this process started by nodejs with 'ipc'?");
    }
}

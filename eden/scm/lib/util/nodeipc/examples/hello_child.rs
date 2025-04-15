/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde_json::Value;
use serde_json::json;

fn main() {
    if let Some(ipc) = nodeipc::get_singleton() {
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

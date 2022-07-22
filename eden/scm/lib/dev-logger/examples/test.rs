/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! This is a test. Run the test with:
//!
//! ```bash,ignore
//! cargo run --example test
//! ```

use std::process::Command;

dev_logger::init!();

const MESSAGE: &str = "hello_log_message_to_test";

fn main() {
    if std::env::args().any(|a| a == MESSAGE) {
        // Run as a child process that might print the MESSAGE.
        tracing::info!(name = MESSAGE);
        return;
    }

    // Test RUST_LOG=info enables tracing::info! messages.
    assert!(run_child("info").contains(MESSAGE));

    // Test RUST_LOG=warn disables tracing::info! messages.
    assert!(!run_child("warn").contains(MESSAGE));
}

fn run_child(rust_log: &str) -> String {
    std::env::set_var("RUST_LOG", rust_log);
    let exe = std::env::current_exe().unwrap();
    let out = Command::new(exe)
        .args(&[MESSAGE])
        .output()
        .expect("failed to execute process");
    String::from_utf8_lossy(&out.stderr).to_string()
}

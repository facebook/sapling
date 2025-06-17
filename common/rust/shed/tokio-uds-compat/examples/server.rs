/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use std::io::Result;

use futures::future::poll_fn;
use tokio::io::AsyncWriteExt;
use tokio_uds_compat::UnixListener;

#[tokio::main]
async fn main() -> Result<()> {
    let path = std::env::temp_dir().join("hello.sock");

    std::fs::remove_file(&path).ok();

    let listener = UnixListener::bind(&path)?;
    println!("listening at {}", path.display());

    loop {
        let (mut conn, addr) = match poll_fn(|cx| listener.poll_accept(cx)).await {
            Err(e) => {
                eprintln!("error while accepting a connection: {:?}", e);
                continue;
            }
            Ok(conn) => conn,
        };
        println!("got a connection from: {:?}", addr);

        if let Err(e) = conn.write(b"HTTP/1.1 200 OK\nContent-Length: 0\n\n").await {
            eprintln!("error while sending response to the client: {:?}", e);
        }
    }
}

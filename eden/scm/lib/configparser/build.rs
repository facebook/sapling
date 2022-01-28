/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::io;
use std::path::Path;

fn check_signed_source() -> io::Result<()> {
    // Check if lib.rs is in sync with the Thrift compiler.
    let manifest_dir_path = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let crate_path = Path::new(&manifest_dir_path);
    let fbcode_path = crate_path.ancestors().nth(4).unwrap();

    let thrift_file_path =
        fbcode_path.join("configerator/structs/scm/hg/hgclientconf/hgclient.thrift");
    if !thrift_file_path.exists() {
        // Do not make it a fatal error on non-fbcode environment (ex. OSS).
        println!(
            "cargo:warning=Does not verify Thrift file at {}.",
            &thrift_file_path.display()
        );
        return Ok(());
    }
    println!(
        "cargo:rerun-if-changed={}",
        &thrift_file_path.to_string_lossy()
    );

    let thrift_file_content = fs::read_to_string(&thrift_file_path)?;

    let hash = "4bc06e1c39884f65a4e9cd145972df39";
    if !thrift_file_content.contains(hash) {
        let msg = format!(
            "thrift_types.rs and HASH need update: {} hash mismatch (expect: {})",
            &thrift_file_path.display(),
            hash,
        );
        return Err(io::Error::new(io::ErrorKind::Other, msg));
    }

    Ok(())
}

fn main() -> io::Result<()> {
    check_signed_source()
}

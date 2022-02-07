/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Works in different environments:
// - fbcode buck build: build.rs not used, generated files not used.
// - fbcode cargo build: build.rs used, runs Thrift compiler, re-generate files.
// - OSS cargo build: build.rs used, does not run Thrift compiler, use existing
//   generated files.

use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

struct ThriftUnit {
    name: &'static str,
    path: &'static str,
    deps: Vec<&'static str>,
}

fn generate_thrift_subcrates(thrift_units: &[ThriftUnit]) -> io::Result<()> {
    // Check if lib.rs is in sync with the Thrift compiler.
    let manifest_dir_path = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let crate_path = Path::new(&manifest_dir_path);
    let fbcode_path = crate_path.ancestors().nth(4).unwrap();

    // Use thrift provided by buck. Currently this only works in fbcode build environment.
    let thrift_bin_path = fbcode_path.join("buck-out/gen/thrift/compiler/thrift");
    let thrift_bin_path = match thrift_bin_path.canonicalize() {
        Ok(path) => path,
        Err(_) => thrift_bin_path,
    };
    if !thrift_bin_path.exists() {
        // Do not make it a fatal error on non-fbcode environment (ex. OSS).
        println!(
            "cargo:warning=Cannot find Thrift compiler at {:?}. Thrift sources are not re-compiled.",
            &thrift_bin_path
        );
        return Ok(());
    }
    println!(
        "cargo:rerun-if-changed={}",
        &thrift_bin_path.to_string_lossy()
    );

    for unit in thrift_units.iter() {
        let subcrate_path = crate_path.join(unit.name);
        fs::create_dir_all(&subcrate_path)?;
        let out_path = subcrate_path.join("src");
        let thrift_source_path = unit.path;
        println!(
            "cargo:rerun-if-changed={}",
            fbcode_path.join(thrift_source_path).to_string_lossy()
        );

        let out = Command::new(&thrift_bin_path)
            .current_dir(fbcode_path)
            .arg("-I")
            .arg(".")
            .arg("-gen")
            .arg("mstch_rust:serde,noserver")
            .arg("-out")
            .arg(&out_path)
            .arg(thrift_source_path)
            .output()
            .unwrap();
        if !out.status.success() {
            panic!(
                "Failed to recompile {:?}: {}{}",
                out_path,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );
        }

        let deps = unit
            .deps
            .iter()
            .map(|name| format!("{name} = {{ path = \"../{name}\" }}", name = name))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(
            subcrate_path.join("Cargo.toml"),
            format!(
                r#"# @{}enerated by thrift-types/build.rs
[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow = "1"
async-trait = "0.1"
const-cstr = "0.3.0"
fbthrift = {{ path = "../../../../../thrift/lib/rust"" }}
futures = "0.3"
serde = {{ version = "1", features = ["derive"] }}
serde_derive = "1.0"
thiserror = "1"
tokio_shim = {{ path = "../../../../../common/rust/shed/tokio_shim" }}
tracing = "0.1"
{}"#,
                "g", unit.name, deps
            ),
        )?;
    }

    Ok(())
}

fn main() -> io::Result<()> {
    let thrift_units = [];

    generate_thrift_subcrates(&thrift_units)
}

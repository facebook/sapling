/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Works in different environments:
// - fbcode buck build: build.rs not used, generated files not used.
// - fbcode cargo build: build.rs used, runs Thrift compiler, re-generate files.
// - OSS cargo build: build.rs used, does not run Thrift compiler, use existing
//   generated files.

use std::fs::{self};
use std::io::{self};
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
        let thrift_source_path = fbcode_path.join(unit.path);
        println!(
            "cargo:rerun-if-changed={}",
            &thrift_source_path.to_string_lossy()
        );

        let out = Command::new(&thrift_bin_path)
            .arg("-I")
            .arg(fbcode_path)
            .arg("-gen")
            .arg("mstch_rust")
            .arg("-out")
            .arg(&out_path)
            .arg(&thrift_source_path)
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
edition = "2018"

[dependencies]
anyhow = "1"
async-trait = "0.1"
fbthrift = {{ path = "../../../../../thrift/lib/rust" }}
futures_preview = {{ package = "futures", version = "0.3" }}
thiserror = "1"
{}"#,
                "g", unit.name, deps
            ),
        )?;
    }

    Ok(())
}

fn main() -> io::Result<()> {
    let thrift_units = [
        ThriftUnit {
            name: "fb303",
            path: "common/fb303/if/fb303.thrift",
            deps: vec!["fb303_core"],
        },
        ThriftUnit {
            name: "fb303_core",
            path: "fb303/thrift/fb303_core.thrift",
            deps: vec![],
        },
        ThriftUnit {
            name: "eden_config",
            path: "eden/fs/config/eden_config.thrift",
            deps: vec![],
        },
        ThriftUnit {
            name: "eden",
            path: "eden/fs/service/eden.thrift",
            deps: vec!["eden_config", "fb303_core"],
        },
    ];

    generate_thrift_subcrates(&thrift_units)
}

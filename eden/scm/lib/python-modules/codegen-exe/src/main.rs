/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Buck build entry point.
//!
//! Input:
//! - --sys-path: Python's sys.path[0].
//! - --out: Output file path (set by buck genrule).
//!
//! Output (file $OUT):
//! - Rust source code including the compiled Python modules.
//!   (compressed Python source code, and uncompressed bytecode)
//!
//! See also codegen::codegen and pycompile.py for details.

use std::env;
use std::path::Path;

fn main() {
    // Simple args parsing.
    let mut sys_path: Option<String> = None;
    let mut out: Option<String> = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--sys-path" => sys_path = Some(args.next().unwrap()),
            "--out" => out = Some(args.next().unwrap()),
            _ => panic!("unknown arg: {arg}"),
        }
    }

    // Run codegen.
    let sys_path = sys_path.as_ref().map(Path::new);
    let out = out.expect("--out is required");
    let code = codegen::generate_code(sys_path).unwrap();

    // Write results.
    std::fs::write(out, code).unwrap();
}

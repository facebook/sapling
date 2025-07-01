/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use std::env;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use thrift_compiler::Config;
use thrift_compiler::GenContext;

/// Calls thrift compiler to produce unified lib.rs from thrift files
#[derive(Parser)]
struct Compiler {
    /// Directory where the result will be saved (default: .)
    #[arg(long, short)]
    out: Option<PathBuf>,

    /// Uses environment variables instead of command line arguments
    #[arg(long = "use", short = 'e')]
    use_environment: bool,

    /// Generation context
    #[arg(long = "context", short = 'g', default_value_t = GenContext::Types)]
    gen_context: GenContext,

    /// Paths to .thrift files
    input: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let args = Compiler::parse();

    let out = args.out.map_or_else(env::current_dir, Result::Ok)?;
    let compiler = if args.use_environment {
        Config::from_env(args.gen_context)?
    } else {
        Config::new(args.gen_context, None, out)?
    };
    compiler.run(args.input)
}

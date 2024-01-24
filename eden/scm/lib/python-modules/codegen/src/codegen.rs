/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

/// Return generated Rust code containing pre-compiled pure Python modules:
///
/// ```ignore
/// // for compatibility check.
/// pub static VERSION_MAJOR: usize = 3;
/// pub static VERSION_MINOR: usize = 10;
/// // modules keyed by name.
/// // uncompressed_source[source_start:source_end] contains source code.
/// pub static MODULES: phf::Map<&str, (&[u8], usize, usize)> = {
///     "mymodule" => (b"bytecode", source_start, source_end),
///     "mymodule.sub" => (b"bytecode", source_start, source_end),
///     ...
/// };
///
/// // zstd compressed Python source code.
/// pub static COMPRESSED_SOURCE: &[u8] = "....";
/// ```
///
/// Input:
/// - `python` is the path to the Python interpreter.
/// - `sys_path` will be inserted to Python's `sys.path[0:0]`.
///   If None, then pycompile.py reads $SYS_ARG0.
pub fn generate_code(python: &Path, sys_path: Option<&Path>) -> String {
    let is_cargo = is_cargo();

    // Run the Python script using the specified Python.
    let mut cmd = if cfg!(windows) && python.to_str().unwrap().contains(' ') {
        // On Windows, buck might pass "python.exe something.par" here.
        // Run it using cmd.exe.
        let cmd = env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string());
        let mut cmd = Command::new(cmd);
        cmd.arg("/c");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.raw_arg(python);
        }
        cmd
    } else {
        Command::new(python)
    };
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped());
    if let Some(path) = sys_path {
        cmd.env("SYS_PATH0", path);
    } else if is_cargo {
        println!("cargo:rerun-if-env-changed=SYS_PATH0");
    }
    cmd.env("ROOT_MODULES", "");
    let mut child = cmd.spawn().unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(PYCOMPILE_SCRIPT.as_bytes()).unwrap();
    }

    // Parse output into ModuleInfos.
    let waited = child.wait_with_output().unwrap();
    if !waited.status.success() {
        panic!("python failed to run: {:#?}", waited.status);
    }
    let output = String::from_utf8(waited.stdout).unwrap();
    let output_lines = output.lines().collect::<Vec<_>>();
    let version_major: usize = output_lines[0].trim().parse().unwrap();
    let version_minor: usize = output_lines[1].trim().parse().unwrap();
    let module_infos: Vec<ModuleInfo> = output_lines[2..]
        .chunks_exact(6)
        .map(ModuleInfo::from_lines)
        .collect();

    if is_cargo {
        for m in &module_infos {
            println!("cargo:rerun-if-changed={}", m.path);
        }
    }

    // Compress Python source code. This saves 10+MB binary size.
    let all_source: Vec<u8> = module_infos
        .iter()
        .map(|m| m.source.as_ref())
        .collect::<Vec<&[u8]>>()
        .concat();
    let compressed_source = zstdelta::diff(b"", &all_source).unwrap();

    // Render the generated code.
    let mut generated_lines = Vec::<String>::new();
    generated_lines.push(format!("// {}enerated by python-modules/codegen.", "@g"));
    generated_lines.push(format!(
        "pub static VERSION_MAJOR: usize = {};",
        version_major
    ));
    generated_lines.push(format!(
        "pub static VERSION_MINOR: usize = {};",
        version_minor
    ));
    generated_lines.push("pub static MODULES: ::phf::Map<&'static str, (&'static str, &'static [u8], bool, usize, usize, bool)> = ::phf::phf_map! {".to_string());
    let mut source_offset = 0;
    for m in module_infos {
        let next_source_offset = source_offset + m.source.len();
        generated_lines.push(format!(
            r#"    "{}" => ("{}\0", b"{}", {}, {}, {}, {}),"#,
            m.name,
            m.name,
            escape_bytes(&m.byte_code),
            m.is_package(),
            source_offset,
            next_source_offset,
            m.is_stdlib,
        ));
        source_offset = next_source_offset;
    }
    generated_lines.push("};".to_string());
    generated_lines.push(format!(
        r#"pub static COMPRESSED_SOURCE: &[u8] = b"{}";"#,
        escape_bytes(&compressed_source)
    ));
    generated_lines.push(String::new());

    generated_lines.join("\n")
}

pub(crate) fn is_cargo() -> bool {
    env::var_os("OUT_DIR").is_some()
}

struct ModuleInfo {
    name: String,
    path: String,
    source: Vec<u8>,
    byte_code: Vec<u8>,
    is_stdlib: bool,
}

impl ModuleInfo {
    fn from_lines(lines: &[&str]) -> Self {
        let name = lines[0].to_string();
        let path = String::from_utf8(from_hex(lines[1].as_bytes())).unwrap();
        let source = from_hex(lines[2].as_bytes());
        let byte_code = from_hex(lines[3].as_bytes());
        let is_stdlib = lines[4].starts_with('T');
        assert!(lines[5].is_empty());
        Self {
            name,
            path,
            source,
            byte_code,
            is_stdlib,
        }
    }

    fn is_package(&self) -> bool {
        self.path.ends_with("__init__.py") || self.path.ends_with("__init__.pyc")
    }
}

fn from_hex(hex: &[u8]) -> Vec<u8> {
    hex.chunks_exact(2)
        .map(|chunk| {
            let s = std::str::from_utf8(chunk).unwrap();
            u8::from_str_radix(s, 16).unwrap()
        })
        .collect()
}

fn escape_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!(r"\x{:02x}", b))
        .collect::<Vec<String>>()
        .concat()
}

const PYCOMPILE_SCRIPT: &str = include_str!("pycompile.py");

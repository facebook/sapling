/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;
use std::path::Path;

use cpython::NoArgs;
use cpython::PyBytes;
use cpython::PyModule;
use cpython::PyResult;
use cpython::Python;

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
/// - The Python interpreter is decided by the rust-cpython crate.
///   It typically respects the `PYTHON_SYS_EXECUTABLE` env var.
/// - `sys_path` will be inserted to Python's `sys.path[0:0]`.
///   If None, then pycompile.py reads $SYS_ARG0.
pub fn generate_code(sys_path: Option<&Path>) -> PyResult<String> {
    let is_cargo = is_cargo();

    // Prepare the module.
    let gil = Python::acquire_gil();
    let py = gil.python();
    let module = PyModule::new(py, "sapling_codegen")?;
    let globals = module.dict(py);
    py.run(PYCOMPILE_SCRIPT, Some(&globals), None)?;

    // Get the version numbers for bytecode ABI check.
    let (version_major, version_minor) = module
        .call(py, "get_version", NoArgs, None)?
        .extract::<(usize, usize)>(py)?;

    // Compile modules.
    let sys_path0 = sys_path.map(|p| p.to_str().unwrap());
    let module_tuples = module
        .call(py, "compile_modules", (sys_path0,), None)?
        .extract::<Vec<ModuleInfoTuple>>(py)?;
    let module_infos: Vec<ModuleInfo> = module_tuples
        .into_iter()
        .map(|t| ModuleInfo::from_tuple(py, t))
        .collect();

    if is_cargo {
        for m in &module_infos {
            // `m.path` could be dummy values like "frozen". Skip them.
            if Path::new(&m.path).exists() {
                println!("cargo:rerun-if-changed={}", m.path);
            }
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

    let generated_code = generated_lines.join("\n");

    Ok(generated_code)
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

type ModuleInfoTuple = (String, String, PyBytes, PyBytes, bool);

impl ModuleInfo {
    fn from_tuple(py: Python, (name, path, source, byte_code, is_stdlib): ModuleInfoTuple) -> Self {
        let source = source.data(py).to_vec();
        let byte_code = byte_code.data(py).to_vec();
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

fn escape_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!(r"\x{:02x}", b))
        .collect::<Vec<String>>()
        .concat()
}

const PYCOMPILE_SCRIPT: &str = include_str!("pycompile.py");

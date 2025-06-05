/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![allow(unexpected_cfgs)]

use std::cell::OnceCell;
use std::env;
use std::ffi::OsString;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::ChildStdin;
use std::process::ChildStdout;
use std::process::Command;
use std::process::Stdio;
use std::str::FromStr;

pub trait SysConfig {}

/// Main struct to obtain Python version and sysconfig.
pub struct PythonSysConfig {
    python: PathBuf,
    repl: OnceCell<PythonREPL>,
}

/// Naive "REPL". Read one line from stdin and execute it. Do not wait for the full stdin.
struct PythonREPL {
    stdout: BufReader<ChildStdout>,
    stdin: ChildStdin,
    _child: Child,
}

impl PythonSysConfig {
    pub fn new() -> Self {
        println!("cargo:rerun-if-env-changed=PYTHON_SYS_EXECUTABLE");
        let python = match env::var_os("PYTHON_SYS_EXECUTABLE") {
            Some(python) => python,
            None => {
                println!("cargo:warning=PYTHON_SYS_EXECUTABLE is recommended at build time");
                OsString::from("python3")
            }
        };
        let python = PathBuf::from(python);
        Self {
            python,
            repl: Default::default(),
        }
    }

    /// Path to the Python interpreter. Might be a relative path.
    pub fn python(&self) -> &Path {
        &self.python
    }

    /// (major, minor)
    pub fn version(&mut self) -> (usize, usize) {
        let repl = self.repl();
        repl.exec("from sys import version_info as v; print(v.major); print(v.minor);\n");
        let major = repl.parse_next();
        let minor = repl.parse_next();
        (major, minor)
    }

    /// Py_ENABLE_SHARED
    pub fn is_static(&mut self) -> bool {
        let repl = self.repl();
        repl.exec("print(__import__('sysconfig').get_config_var('Py_ENABLE_SHARED'));\n");
        let out = repl.read_next_line();
        out.starts_with("0")
    }

    pub fn cflags(&mut self) -> String {
        let repl = self.repl();
        repl.exec("print(__import__('sysconfig').get_config_var('CFLAGS') or '');\n");
        repl.read_next_line()
    }

    pub fn ldflags(&mut self) -> String {
        let repl = self.repl();
        repl.exec("print(__import__('sysconfig').get_config_var('LDFLAGS') or '');\n");
        repl.read_next_line()
    }

    pub fn include(&mut self) -> String {
        let repl = self.repl();
        repl.exec("print(__import__('sysconfig').get_paths()['include'].strip());\n");
        repl.read_next_line()
    }

    pub fn headers(&mut self) -> String {
        let repl = self.repl();
        repl.exec("print(__import__('sysconfig').get_paths().get('headers') or '');\n");
        repl.read_next_line()
    }

    fn repl(&mut self) -> &mut PythonREPL {
        self.repl.get_or_init(|| PythonREPL::new(&self.python));
        self.repl.get_mut().unwrap()
    }
}

impl PythonREPL {
    /// Spawn a new Python process to read sysconfigs.
    fn new(python: &Path) -> Self {
        let mut child = Command::new(python)
            .args(["-Suc", "[exec(c) for c in __import__('sys').stdin];"])
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()
            .expect("spawn python");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            stdout,
            stdin,
            _child: child,
        }
    }

    /// Execute one-line Python script.
    fn exec(&mut self, script: &str) {
        assert!(script.ends_with("\n"));
        self.stdin.write_all(script.as_bytes()).unwrap();
    }

    /// Read one line from Python stdout. Newline at the end is stripped.
    fn read_next_line(&mut self) -> String {
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        while line.ends_with('\n') || line.ends_with('\r') {
            line.pop();
        }
        line
    }

    /// Read one line from Python stdout and parse to T.
    fn parse_next<T: FromStr>(&mut self) -> T {
        let line = self.read_next_line();
        line.parse().ok().unwrap()
    }
}

// Not needed for buck test.
#[cfg(all(test, not(fbcode_build)))]
mod tests {
    use super::*;

    #[test]
    fn test_basic() {
        let mut sysconfig = PythonSysConfig::new();
        let (major, minor) = sysconfig.version();
        let is_static = sysconfig.is_static();
        assert!(major >= 3 || minor >= 8);
        eprintln!("major: {major}, minor: {minor}, is_static: {is_static}");
    }
}

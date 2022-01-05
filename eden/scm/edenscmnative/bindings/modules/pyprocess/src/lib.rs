/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;
use std::fs;
use std::io;
use std::process::Child as RustChild;
use std::process::Command as RustCommand;
use std::process::ExitStatus as RustExitStatus;
use std::process::Stdio as RustStdio;

use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::ResultPyErrExt;
use spawn_ext::CommandExt;

py_class!(class Command |py| {
    data inner: RefCell<RustCommand>;

    /// Constructs a new Command for launching the program at path program, with
    /// the following default configuration:
    /// - No arguments to the program
    /// - Inherit the current process's environment
    /// - Inherit the current process's working directory
    /// - Inherit stdin/stdout/stderr for spawn or status, but create pipes for output
    @staticmethod
    def new(program: String) -> PyResult<Self> {
        let command = RustCommand::new(program);
        Self::create_instance(py, RefCell::new(command))
    }

    /// Adds an argument to pass to the program.
    def arg(&self, arg: &str) -> PyResult<Self> {
        self.mutate_then_clone(py, |c| c.arg(arg))
    }

    /// Adds multiple arguments to pass to the program.
    def args(&self, args: Vec<String>) -> PyResult<Self> {
        self.mutate_then_clone(py, |c| c.args(args))
    }

    /// Inserts or updates an environment variable mapping.
    def env(&self, key: &str, val: &str) -> PyResult<Self> {
        self.mutate_then_clone(py, |c| c.env(key, val))
    }

    /// Adds or updates multiple environment variable mappings.
    def envs(&self, items: Vec<(String, String)>) -> PyResult<Self> {
        self.mutate_then_clone(py, |c| c.envs(items))
    }

    /// Clears the entire environment map for the child process.
    def envclear(&self) -> PyResult<Self> {
        self.mutate_then_clone(py, |c| c.env_clear())
    }

    /// Sets the working directory for the child process.
    def currentdir(&self, dir: &PyPath) -> PyResult<Self> {
        self.mutate_then_clone(py, |c| c.current_dir(dir))
    }

    /// Configuration for the child process's standard input (stdin) handle.
    def stdin(&self, cfg: Stdio) -> PyResult<Self> {
        let f = cfg.to_rust(py).map_pyerr(py)?;
        self.mutate_then_clone(py, |c| c.stdin(f))
    }

    /// Configuration for the child process's standard output (stdout) handle.
    def stdout(&self, cfg: Stdio) -> PyResult<Self> {
        let f = cfg.to_rust(py).map_pyerr(py)?;
        self.mutate_then_clone(py, |c| c.stdout(f))
    }

    /// Configuration for the child process's standard error (stderr) handle.
    def stderr(&self, cfg: Stdio) -> PyResult<Self> {
        let f = cfg.to_rust(py).map_pyerr(py)?;
        self.mutate_then_clone(py, |c| c.stderr(f))
    }

    /// Attempt to avoid inheriting file handles.
    /// Call this before setting up redirections.
    def avoidinherithandles(&self) -> PyResult<Self> {
        self.mutate_then_clone(py, |c| c.avoid_inherit_handles())
    }

    /// Use new session or process group.
    /// Call this after avoidinherithandles.
    def newsession(&self) -> PyResult<Self> {
        self.mutate_then_clone(py, |c| c.new_session())
    }

    /// Executes the command as a child process, returning a handle to it.
    def spawn(&self) -> PyResult<Child> {
        // This is safer than `os.fork()` in Python because Python cannot
        // interrupt between `fork()` and `exec()` due to Rust holding GIL.
        let mut inner = self.inner(py).borrow_mut();
        let child = inner.spawn().map_pyerr(py)?;
        Child::from_rust(py, child)
    }

    /// Spawn the process then forget about it.
    /// File handles are not inherited. stdio will be redirected to /dev/null.
    def spawndetached(&self) -> PyResult<Child> {
        // This is safer than `os.fork()` in Python because Python cannot
        // interrupt between `fork()` and `exec()` due to Rust holding GIL.
        let mut inner = self.inner(py).borrow_mut();
        let child = inner.spawn_detached().map_pyerr(py)?;
        Child::from_rust(py, child)
    }

});

impl Command {
    /// Make changes to `inner`, then clone self.
    fn mutate_then_clone(
        &self,
        py: Python,
        func: impl FnOnce(&mut RustCommand) -> &mut RustCommand,
    ) -> PyResult<Self> {
        let mut inner = self.inner(py).borrow_mut();
        func(&mut inner);
        Ok(self.clone_ref(py))
    }
}

py_class!(class Stdio |py| {
    data inner: Box<dyn Fn() -> io::Result<RustStdio> + Send + 'static> ;

    /// A new pipe should be arranged to connect the parent and child processes.
    @staticmethod
    def piped() -> PyResult<Self> {
        Self::create_instance(py, Box::new(|| Ok(RustStdio::piped())))
    }

    /// The child inherits from the corresponding parent descriptor.
    @staticmethod
    def inherit() -> PyResult<Self> {
        Self::create_instance(py, Box::new(|| Ok(RustStdio::inherit())))
    }

    /// This stream will be ignored. This is the equivalent of attaching the
    /// stream to /dev/null.
    @staticmethod
    def null() -> PyResult<Self> {
        Self::create_instance(py, Box::new(|| Ok(RustStdio::null())))
    }

    /// Open a file as `Stdio`.
    @staticmethod
    def open(path: &PyPath, read: bool = false, write: bool = false, create: bool = false, append: bool = false) -> PyResult<Self> {
        let path = path.to_path_buf();
        Self::create_instance(py, Box::new(move || {
            let mut opts = fs::OpenOptions::new();
            let file = opts.write(write).read(read).create(create).append(append).open(&path)?;
            Ok(file.into())
        }))
    }
});

impl Stdio {
    fn to_rust(&self, py: Python) -> io::Result<RustStdio> {
        self.inner(py)()
    }
}

py_class!(class Child |py| {
    data inner: RefCell<RustChild>;

    /// Forces the child process to exit. If the child has already exited, an
    /// InvalidInput error is returned.
    def kill(&self) -> PyResult<PyNone> {
        let mut inner = self.inner(py).borrow_mut();
        inner.kill().map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Returns the OS-assigned process identifier associated with this child.
    def id(&self) -> PyResult<u32> {
        let inner = self.inner(py).borrow();
        Ok(inner.id())
    }

    /// Waits for the child to exit completely, returning the status that it
    /// exited with. This function will continue to have the same return value
    /// after it has been called at least once.
    def wait(&self) -> PyResult<ExitStatus> {
        let mut inner = self.inner(py).borrow_mut();
        let status = inner.wait().map_pyerr(py)?;
        ExitStatus::from_rust(py, status)
    }

    /// Attempts to collect the exit status of the child if it has already exited.
    def try_wait(&self) -> PyResult<Option<ExitStatus>> {
        let mut inner = self.inner(py).borrow_mut();
        match inner.try_wait().map_pyerr(py)? {
            Some(s) => Ok(Some(ExitStatus::from_rust(py, s)?)),
            None => Ok(None)
        }
    }
});

impl Child {
    fn from_rust(py: Python, child: RustChild) -> PyResult<Self> {
        Self::create_instance(py, RefCell::new(child))
    }
}

py_class!(class ExitStatus |py| {
    data inner: RustExitStatus;

    /// Was termination successful? Signal termination is not considered a
    /// success, and success is defined as a zero exit status.
    def success(&self) -> PyResult<bool> {
        Ok(self.inner(py).success())
    }

    /// Returns the exit code of the process, if any.
    /// On Unix, this will return None if the process was terminated by a signal.
    def code(&self) -> PyResult<Option<i32>> {
        Ok(self.inner(py).code())
    }
});

impl ExitStatus {
    fn from_rust(py: Python, status: RustExitStatus) -> PyResult<Self> {
        Self::create_instance(py, status)
    }
}

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "process"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<Child>(py)?;
    m.add_class::<Command>(py)?;
    m.add_class::<Stdio>(py)?;
    Ok(m)
}

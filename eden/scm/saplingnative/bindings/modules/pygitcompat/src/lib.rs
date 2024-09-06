/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::path::PathBuf;
use std::sync::Arc;

use configmodel::Config;
use cpython::*;
use cpython_ext::convert::ImplInto;
use cpython_ext::convert::Serde;
use cpython_ext::ResultPyErrExt;
use gitcompat::rungit::BareGit as RustBareGit;
use gitcompat::rungit::GitCmd;
use pyprocess::Command as PyCommand;
use pyprocess::ExitStatus as PyExitStatus;
use pyprocess::Output as PyOutput;
use types::HgId;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "gitcompat"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<BareGit>(py)?;
    Ok(m)
}

py_class!(pub class BareGit |py| {
    data inner: RustBareGit;

    def __new__(_cls, gitdir: String, config: ImplInto<Arc<dyn Config>>) -> PyResult<Self> {
        let git = RustBareGit::from_git_dir_and_config(gitdir.into(), &config.into());
        Self::create_instance(py, git)
    }

    /// git_cmd(cmd_name, args) -> Command
    def git_cmd(&self, cmd_name: &str, args: Vec<String>) -> PyResult<PyCommand> {
        let cmd = self.inner(py).git_cmd(cmd_name, &args);
        PyCommand::from_rust(py, cmd)
    }

    /// call(cmd_name, args) -> Output. Raise if exit code is not 0.
    def call(&self, cmd_name: &str, args: Vec<String>) -> PyResult<PyOutput> {
        let output = self.inner(py).call(cmd_name, &args).map_pyerr(py)?;
        PyOutput::from_rust(py, output)
    }

    /// run(cmd_name, args) -> ExitStatus. Raise if exit code is not 0.
    def run(&self, cmd_name: &str, args: Vec<String>) -> PyResult<PyExitStatus> {
        let status = self.inner(py).run(cmd_name, &args).map_pyerr(py)?;
        PyExitStatus::from_rust(py, status)
    }

    /// resolve_head() -> node
    def resolve_head(&self) -> PyResult<Serde<HgId>> {
        let id = self.inner(py).resolve_head().map_pyerr(py)?;
        Ok(Serde(id))
    }

    /// translate_git_config() -> Tuple[str, str]
    def translate_git_config(&self) -> PyResult<(String, String)> {
        self.inner(py).translate_git_config().map_pyerr(py)
    }
});

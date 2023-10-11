/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::ResultPyErrExt;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "webview"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "open_isl",
        py_fn!(py, open_isl(
            repo_cwd: &str,
            port: i32,
            no_open: bool,
            json_output: bool,
            foreground: bool,
            force: bool,
            kill: bool,
            platform: &str,
            slcommand: &str,
            slversion: &str,
            server_cwd: &str,
            nodepath: &str,
            entrypoint: &str,
            browser: Option<&str>,
            no_app: bool,
        )),
    )?;
    Ok(m)
}

fn open_isl(
    py: Python,
    repo_cwd: &str,
    port: i32,
    no_open: bool,
    json: bool,
    foreground: bool,
    force: bool,
    kill: bool,
    platform: &str,
    slcommand: &str,
    slversion: &str,
    server_cwd: &str,
    nodepath: &str,
    entrypoint: &str,
    browser: Option<&str>,
    no_app: bool,
) -> PyResult<PyNone> {
    let opts = webview_app::ISLSpawnOptions {
        repo_cwd: repo_cwd.to_string(),
        port,
        no_open,
        json,
        foreground,
        force,
        kill,
        platform: platform.to_string(),
        slcommand: slcommand.to_string(),
        slversion: slversion.to_string(),
        server_cwd: server_cwd.to_string(),
        nodepath: nodepath.to_string(),
        entrypoint: entrypoint.to_string(),
        browser: browser.map(str::to_string),
        no_app,
    };
    webview_app::open_isl(opts).map_pyerr(py)?;
    Ok(PyNone)
}

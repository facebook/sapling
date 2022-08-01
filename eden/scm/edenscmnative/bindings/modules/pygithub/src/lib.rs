/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Python bindings for prepared queries to GitHub's GraphQL API.

use cpython::*;
use cpython_ext::convert::Serde;
use cpython_ext::ResultPyErrExt;
use github::GitHubRepo;
use github::PullRequest;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "github"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "get_pull_request",
        py_fn!(
            py,
            get_pull_request(token: &str, owner: &str, name: &str, number: u32)
        ),
    )?;
    Ok(m)
}

fn get_pull_request(
    py: Python,
    token: &str,
    owner: &str,
    name: &str,
    number: u32,
) -> PyResult<Option<Serde<PullRequest>>> {
    let repo = GitHubRepo {
        owner: owner.to_string(),
        name: name.to_string(),
    };
    github::get_pull_request(token, &repo, number)
        .map_pyerr(py)?
        .map_or_else(|| Ok(None), |pull_request| Ok(Some(Serde(pull_request))))
}

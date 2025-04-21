/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::bail;
use factory::FunctionSignature;
use hook::PythonHookSig;
use repo::Repo;

use crate::HgPython;

/// Register a function to run python hooks.
pub(crate) fn init() {
    factory::register_function::<PythonHookSig>(initialize_python_and_run_python_hook);
}

fn initialize_python_and_run_python_hook(
    input: <PythonHookSig as FunctionSignature>::In,
) -> Result<i8> {
    // Initialize the Python interpreter on demand.
    let _python = HgPython::new(&[]);

    // Prepare input. Downcast is to avoid lib/hook depending on lib/repo.
    let (optional_repo, spec, hook_name, kwargs) = input;
    let repo = match optional_repo {
        Some(repo) => match repo.downcast_ref::<Repo>() {
            None => bail!("bug: unexpected Repo type"),
            Some(v) => Some(v),
        },
        None => None,
    };
    pyhook::run_python_hook(repo, spec, hook_name, kwargs)
}
